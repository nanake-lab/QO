use crate::{
    tree::Tree,
    wire::{Wire, invalid},
};
use qo_harness::{EventSink, Harness, HarnessEvent, HarnessFactory, Reply, Started, StreamText};
use qo_protocol::{ApprovalKind, Choice, SessionOpenParams, Task};
use serde_json::{Map, Value, json};
use std::{
    env, fs,
    io::{self, BufRead, BufReader, Read, Write},
    path::{Path, PathBuf},
    process::{ChildStdin, Command, Stdio},
    sync::{Arc, Mutex, mpsc},
    thread,
    time::{Duration, Instant},
};

// Bound a stalled version query or missing local RPC reply, independently of model turn duration.
const RPC_TIMEOUT: Duration = Duration::from_secs(30);
const SYSTEM_ENV: &[&str] = &[
    "PATH",
    "PATHEXT",
    "HOME",
    "USERPROFILE",
    "SystemRoot",
    "WINDIR",
    "COMSPEC",
    "TEMP",
    "TMP",
    "TMPDIR",
    "LANG",
    "LC_ALL",
    "LC_CTYPE",
    "APPDATA",
    "LOCALAPPDATA",
];
const CREDENTIALS: &[&str] = &["OPENAI_API_KEY"];
pub struct CodexFactory {
    credentials: Vec<(&'static str, String)>,
    executable: PathBuf,
    home: PathBuf,
    finish: Duration,
}
impl CodexFactory {
    pub fn new(executable: PathBuf, state: &Path, finish: Duration) -> Self {
        Self {
            credentials: CREDENTIALS
                .iter()
                .filter_map(|&key| env::var(key).ok().map(|value| (key, value)))
                .collect(),
            executable,
            home: state.join("codex"),
            finish,
        }
    }
    fn executable(&self) -> io::Result<PathBuf> {
        if self.executable.components().count() != 1 {
            return fs::canonicalize(&self.executable);
        }
        let mut name = self.executable.clone();
        if cfg!(windows) && name.extension().is_none() {
            name.set_extension("exe");
        }
        let path = env::var_os("PATH").unwrap_or_default();
        for dir in env::split_paths(&path) {
            let candidate = dir.join(&name);
            if candidate.is_file() {
                return fs::canonicalize(candidate);
            }
        }
        Err(io::Error::new(
            io::ErrorKind::NotFound,
            "Codex executable missing from PATH",
        ))
    }
    fn spawn(
        &self,
        executable: &Path,
        args: &[&str],
        task: &Task,
        events: &EventSink,
    ) -> io::Result<Arc<Mutex<Tree>>> {
        let mut command = Command::new(executable);
        command
            .env_clear()
            .env("CODEX_HOME", &self.home)
            .current_dir(&task.cwd);
        for name in SYSTEM_ENV {
            if let Some(value) = env::var_os(name) {
                command.env(name, value);
            }
        }
        for (name, value) in &self.credentials {
            command.env(name, value);
        }
        command
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut tree = Tree::spawn(command)?;
        let stderr = tree.child().stderr.take().unwrap();
        let secrets = events.secrets.clone();
        thread::Builder::new()
            .name("codex-stderr".into())
            .spawn(move || {
                let mut pending = StreamText::default();
                let mut reader = BufReader::new(stderr);
                let mut line = Vec::new();
                while let Ok(n) = reader.read_until(b'\n', &mut line) {
                    if n == 0 {
                        break;
                    }
                    let text = pending.push(&secrets, &String::from_utf8_lossy(&line));
                    let _ = io::stderr().lock().write_all(text.as_bytes());
                    line.clear();
                }
                let _ = io::stderr()
                    .lock()
                    .write_all(pending.finish(&secrets).as_bytes());
            })?;
        let shared = Arc::new(Mutex::new(tree));
        let weak = Arc::downgrade(&shared);
        events.on_cancel(move || {
            if let Some(t) = weak.upgrade() {
                t.lock().unwrap().kill();
            }
        })?;
        Ok(shared)
    }
    fn version(&self, executable: &Path, task: &Task, sink: &EventSink) -> io::Result<String> {
        let lock: Value = serde_json::from_str(include_str!("../../../config/harness.lock.json"))?;
        let expected = lock["codex"]["version"].as_str().unwrap();
        let tree = self.spawn(executable, &["--version"], task, sink)?;
        let stdout = tree.lock().unwrap().child().stdout.take().unwrap();
        let (tx, rx) = mpsc::sync_channel(1);
        thread::spawn(move || {
            let mut s = String::new();
            let res = stdout.take(512).read_to_string(&mut s).map(|_| s);
            let _ = tx.send(res);
        });
        let report = rx
            .recv_timeout(RPC_TIMEOUT)
            .map_err(|_| io::Error::other("Codex version query timed out"))??;
        let deadline = Instant::now() + RPC_TIMEOUT;
        let status = loop {
            if let Some(status) = tree.lock().unwrap().child().try_wait()? {
                break status;
            }
            if Instant::now() >= deadline {
                return Err(io::Error::other("Codex version query timed out"));
            }
            thread::sleep(Duration::from_millis(10));
        };
        let actual = report
            .trim()
            .strip_prefix("codex-cli ")
            .unwrap_or(report.trim());
        if !status.success() || report.trim() != format!("codex-cli {expected}") {
            return Err(io::Error::other(format!(
                "Codex version mismatch: expected {expected}, actual {actual}"
            )));
        }
        Ok(expected.into())
    }
}

type Input = Arc<Mutex<Option<ChildStdin>>>;
fn send(stdin: &Input, frame: &Value) -> io::Result<()> {
    let mut guard = stdin.lock().unwrap();
    let out = guard
        .as_mut()
        .ok_or_else(|| io::Error::other("Codex stdin closed"))?;
    serde_json::to_writer(&mut *out, frame)?;
    out.write_all(b"\n")?;
    out.flush()
}
struct Request {
    id: u64,
    start: Option<String>,
    done: mpsc::SyncSender<io::Result<Value>>,
}
struct State {
    pending: Option<Request>,
    wire: Wire,
}
struct Process {
    tree: Arc<Mutex<Tree>>,
    stdin: Input,
    state: Arc<Mutex<State>>,
    next: u64,
    finish: Duration,
}
impl Process {
    fn connect(
        tree: Arc<Mutex<Tree>>,
        sink: EventSink,
        session: String,
        thread_id: String,
        finish: Duration,
        next: u64,
    ) -> io::Result<Self> {
        let (stdin, stdout) = {
            let mut tree = tree.lock().unwrap();
            let child = tree.child();
            (
                Arc::new(Mutex::new(child.stdin.take())),
                child.stdout.take().unwrap(),
            )
        };
        let state = Arc::new(Mutex::new(State {
            pending: None,
            wire: Wire::new(session, thread_id),
        }));
        let proc = Process {
            tree,
            stdin: stdin.clone(),
            state: state.clone(),
            next,
            finish,
        };
        thread::Builder::new()
            .name("codex-jsonl".into())
            .spawn(move || {
                let run = || -> io::Result<()> {
                    for line in BufReader::new(stdout).lines() {
                        frame(&state, &stdin, &sink, serde_json::from_str(&line?)?)?;
                    }
                    Ok(())
                };
                let event = match run() {
                    Ok(()) => HarnessEvent::Exited,
                    Err(e) => {
                        HarnessEvent::Failed(if e.to_string() == "unsupported Codex callback" {
                            e.to_string()
                        } else {
                            "invalid Codex frame".into()
                        })
                    }
                };
                if let Some(r) = state.lock().unwrap().pending.take() {
                    let _ = r.done.send(Err(io::Error::other("Codex reader stopped")));
                }
                let _ = sink.send(event);
            })?;
        Ok(proc)
    }
    fn request(&mut self, method: &str, params: Value, start: Option<String>) -> io::Result<Value> {
        self.next += 1;
        let (done, rx) = mpsc::sync_channel(1);
        self.state.lock().unwrap().pending = Some(Request {
            id: self.next,
            start,
            done,
        });
        send(
            &self.stdin,
            &json!({"method":method,"params":params,"id":self.next}),
        )?;
        rx.recv_timeout(RPC_TIMEOUT)
            .map_err(|_| io::Error::other("missing Codex RPC reply"))?
    }
    fn remote(&self, local: &str) -> io::Result<(String, String)> {
        let s = self.state.lock().unwrap();
        let (_, remote) = s
            .wire
            .turn
            .as_ref()
            .filter(|(t, _)| t == local)
            .ok_or_else(invalid)?;
        Ok((s.wire.thread.clone(), remote.clone()))
    }
}
fn frame(state: &Mutex<State>, stdin: &Input, sink: &EventSink, v: Value) -> io::Result<()> {
    if let Some(method) = v["method"].as_str() {
        if let Some(id) = v.get("id") {
            send(
                stdin,
                &json!({"id":id,"error":{"code":-32601,"message":"unsupported callback"}}),
            )?;
            return Err(io::Error::other("unsupported Codex callback"));
        }
        let mut s = state.lock().unwrap();
        if !s.wire.thread.is_empty() {
            if method == "turn/started"
                && let Some(local) = s.pending.as_ref().and_then(|r| r.start.clone())
            {
                let remote = v["params"]["turn"]["id"].as_str().ok_or_else(invalid)?;
                s.wire.begin(local, remote.into());
            }
            s.wire.notify(method, &v["params"], sink)?;
        }
    } else {
        let mut s = state.lock().unwrap();
        let req = s.pending.take().ok_or_else(invalid)?;
        if v["id"].as_u64() != Some(req.id) {
            return Err(invalid());
        }
        if v.get("error").is_some() {
            let _ = req.done.send(Err(io::Error::other("Codex rejected RPC")));
        } else {
            let r = v.get("result").ok_or_else(invalid)?;
            if let Some(local) = req.start {
                let remote = r["turn"]["id"].as_str().ok_or_else(invalid)?;
                if s.wire.turn.as_ref() != Some(&(local.clone(), remote.to_owned())) {
                    s.wire.begin(local, remote.into());
                }
            }
            let _ = req.done.send(Ok(r.clone()));
        }
    }
    Ok(())
}

impl HarnessFactory for CodexFactory {
    fn register_secrets(&self, secrets: &qo_harness::Secrets) {
        for (_, value) in &self.credentials {
            secrets.remember(value);
        }
    }
    fn name(&self) -> &str {
        "codex"
    }
    fn start(
        &self,
        task: &Task,
        params: &SessionOpenParams,
        sink: EventSink,
    ) -> io::Result<Started> {
        self.register_secrets(&sink.secrets);
        fs::create_dir_all(&self.home)?;
        let executable = self.executable()?;
        let version = self.version(&executable, task, &sink)?;
        let tree = self.spawn(&executable, &["app-server"], task, &sink)?;
        let mut proc = Process::connect(
            tree,
            sink,
            params.session_id.clone(),
            String::new(),
            self.finish,
            0,
        )?;
        proc.request(
            "initialize",
            json!({"clientInfo":{"name":"qo","version":env!("CARGO_PKG_VERSION")}}),
            None,
        )?;
        send(&proc.stdin, &json!({"method":"initialized"}))?;
        let mut options = json!({"cwd":task.cwd,"approvalPolicy":"untrusted","approvalsReviewer":"user","sandbox":"read-only"});
        if let Some(v) = &params.model {
            options["model"] = json!(v);
        }
        if let Some(v) = &params.reasoning_effort {
            options["config"] = json!({"model_reasoning_effort":v});
        }
        if let Some(v) = &params.service_tier {
            options["serviceTier"] = json!(v);
        }
        let r = proc.request("thread/start", options, None)?;
        verify(&r, Path::new(&task.cwd))?;
        proc.state.lock().unwrap().wire.thread =
            r["thread"]["id"].as_str().ok_or_else(invalid)?.into();
        let setting = |key: &str| r[key].as_str().map(str::to_owned);
        Ok(Started {
            process: Box::new(proc),
            version,
            model: setting("model"),
            reasoning_effort: setting("reasoningEffort"),
            service_tier: setting("serviceTier"),
        })
    }
}
fn verify(r: &Value, cwd: &Path) -> io::Result<()> {
    if r["approvalPolicy"] != "untrusted"
        || r["approvalsReviewer"] != "user"
        || r["sandbox"]["type"] != "readOnly"
        || !matches!(
            r["sandbox"].get("networkAccess"),
            None | Some(Value::Bool(false))
        )
        || fs::canonicalize(r["cwd"].as_str().ok_or_else(invalid)?)? != fs::canonicalize(cwd)?
    {
        return Err(io::Error::other(
            "Codex effective approval, sandbox or cwd mismatch",
        ));
    }
    Ok(())
}
impl Harness for Process {
    fn start_turn(&mut self, turn: &str, input: &str) -> io::Result<()> {
        let thread = self.state.lock().unwrap().wire.thread.clone();
        self.request(
            "turn/start",
            json!({"threadId":thread,"input":[{"type":"text","text":input}]}),
            Some(turn.into()),
        )?;
        Ok(())
    }
    fn append(&mut self, turn: &str, input: &str) -> io::Result<()> {
        let (thread, id) = self.remote(turn)?;
        self.request(
            "turn/steer",
            json!({"threadId":thread,"expectedTurnId":id,"input":[{"type":"text","text":input}]}),
            None,
        )?;
        Ok(())
    }
    fn interrupt(&mut self, turn: &str) -> io::Result<()> {
        let (thread, id) = self.remote(turn)?;
        self.request(
            "turn/interrupt",
            json!({"threadId":thread,"turnId":id}),
            None,
        )?;
        Ok(())
    }
    fn valid_decision(
        &self,
        _: &str,
        _: ApprovalKind,
        _: Choice,
        _: Option<&Map<String, Value>>,
    ) -> bool {
        false
    }
    fn reply(&mut self, _: &str, _: Reply) -> io::Result<()> {
        Err(io::Error::other("unsupported Codex callback"))
    }
    fn finish(&mut self) -> io::Result<()> {
        self.stdin.lock().unwrap().take();
        let start = Instant::now();
        while start.elapsed() < self.finish {
            if let Some(status) = self.tree.lock().unwrap().child().try_wait()? {
                return if status.success() {
                    Ok(())
                } else {
                    Err(io::Error::other("Codex exited unsuccessfully"))
                };
            }
            thread::sleep(Duration::from_millis(20)); // Bound wait granularity without a busy loop.
        }
        self.terminate();
        Ok(())
    }
    fn terminate(&mut self) {
        self.tree.lock().unwrap().kill();
    }
}

#[cfg(test)]
#[path = "adapter_tests.rs"]
mod tests;
