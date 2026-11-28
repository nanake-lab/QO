use crate::wire::{Client, Secrets, http};
use serde_json::{Value, json};
use std::{
    fs,
    net::{TcpListener, TcpStream},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, Instant},
};
use tempfile::TempDir;

/// Overrides apply only to the child process. None preserves its normal defaults.
#[derive(Clone, Copy, Default)]
pub struct Options {
    pub login_seconds: Option<u64>,
    pub approval_seconds: Option<u64>,
    pub retain_events: Option<usize>,
    pub pause_delivery: bool,
    pub capture_stderr: bool,
}

/// Configure a child and identify its discovery directory. Restart uses the same root and port.
pub trait Target {
    fn command(&self, root: &Path, port: u16, options: Options) -> Command;
    fn discovery(&self, root: &Path) -> PathBuf;
}
pub struct Real(pub &'static str);
pub struct Mock(pub &'static str);
pub struct ScriptedReal(pub &'static str);

pub fn isolated(root: &Path, binary: &str) -> Command {
    let mut c = Command::new(binary);
    c.env("LOCALAPPDATA", root)
        .env("HOME", root)
        .env("XDG_RUNTIME_DIR", root.join("run"))
        .env("XDG_CONFIG_HOME", root.join("config"))
        .env("XDG_STATE_HOME", root.join("state"))
        .env("XDG_DATA_HOME", root.join("data"));
    c
}

impl Target for Real {
    fn command(&self, root: &Path, port: u16, o: Options) -> Command {
        let config = root.join(if cfg!(windows) {
            "QO/config"
        } else {
            "config/qo"
        });
        fs::create_dir_all(&config).unwrap();
        let mut settings = json!({"port":port});
        if let Some(n) = o.login_seconds {
            settings["loginLinkSeconds"] = json!(n);
        }
        if let Some(n) = o.approval_seconds {
            settings["approvalTtlSeconds"] = json!(n);
        }
        fs::write(config.join("settings.json"), settings.to_string()).unwrap();
        isolated(root, self.0)
    }
    fn discovery(&self, root: &Path) -> PathBuf {
        root.join(if cfg!(windows) { "QO/run" } else { "run/qo" })
    }
}

impl Target for ScriptedReal {
    fn command(&self, root: &Path, port: u16, options: Options) -> Command {
        let mut cmd = Real(self.0).command(root, port, options);
        cmd.arg("--test-harness");
        if let Some(n) = options.retain_events {
            cmd.env("QO_TEST_RETAIN_EVENTS", n.to_string());
        }
        if options.pause_delivery {
            cmd.env("QO_TEST_DELIVERY_MARKER", root.join("before-delivery"));
        }
        cmd
    }
    fn discovery(&self, root: &Path) -> PathBuf {
        Real(self.0).discovery(root)
    }
}

impl Target for Mock {
    fn command(&self, root: &Path, port: u16, o: Options) -> Command {
        let web = root.join("web");
        fs::create_dir_all(&web).unwrap();
        fs::write(web.join("index.html"), "QO").unwrap();
        let mut c = isolated(root, self.0);
        c.arg("--root")
            .arg(root.join("mock"))
            .arg("--web")
            .arg(web)
            .arg("--port")
            .arg(port.to_string());
        if let Some(n) = o.login_seconds {
            c.arg("--login-link-seconds").arg(n.to_string());
        }
        if let Some(n) = o.approval_seconds {
            c.arg("--approval-ttl-seconds").arg(n.to_string());
        }
        if let Some(n) = o.retain_events {
            c.arg("--retain-events").arg(n.to_string());
        }
        c
    }
    fn discovery(&self, root: &Path) -> PathBuf {
        root.join("mock/run")
    }
}

struct Process(Child);
impl Drop for Process {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

pub struct Instance<'a> {
    process: Option<Process>,
    root: TempDir,
    target: &'a dyn Target,
    options: Options,
    pub port: u16,
    secrets: Secrets,
    native: String,
    desktop: String,
}
impl<'a> Instance<'a> {
    pub fn start(target: &'a dyn Target, options: Options) -> Self {
        let port = TcpListener::bind(("127.0.0.1", 0))
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        let mut s = Self {
            process: None,
            root: tempfile::tempdir().unwrap(),
            target,
            options,
            port,
            secrets: Secrets::default(),
            native: String::new(),
            desktop: String::new(),
        };
        s.restart();
        s
    }
    pub fn root(&self) -> &Path {
        self.root.path()
    }
    pub fn origin(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }
    pub fn restart(&mut self) {
        drop(self.process.take());
        let mut cmd = self.target.command(self.root(), self.port, self.options);
        if self.options.capture_stderr {
            cmd.stderr(Stdio::piped());
        }
        let mut child = Process(cmd.stdout(Stdio::null()).spawn().unwrap());
        // Connect does not prove publication; obtain an HTTP response from the running router.
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            assert!(
                child.0.try_wait().unwrap().is_none(),
                "service exited during startup"
            );
            if TcpStream::connect(("127.0.0.1", self.port)).is_ok() {
                break;
            }
            assert!(Instant::now() < deadline, "service startup timed out");
            thread::sleep(Duration::from_millis(20));
        }
        let host = format!("127.0.0.1:{}", self.port);
        assert_eq!(http(self.port, "GET /", &[("Host", &host)], "").0, 200);
        let dir = self.target.discovery(self.root());
        let read = |name: &str| {
            let record: Value = serde_json::from_slice(&fs::read(dir.join(name)).unwrap()).unwrap();
            assert_eq!(record["port"], self.port);
            let token = record["token"].as_str().unwrap().to_owned();
            self.secrets.add(&token);
            token
        };
        self.native = read("endpoint.json");
        self.desktop = read("desktop.json");
        self.process = Some(child);
    }
    pub(crate) fn wait_for_failure(&mut self) -> String {
        let child = &mut self.process.as_mut().unwrap().0;
        // Observe the child's own exit before restart(), which would otherwise kill it.
        let deadline = Instant::now() + Duration::from_secs(8);
        loop {
            if let Some(status) = child.try_wait().unwrap() {
                assert!(status.code().is_some_and(|code| code != 0), "{status}");
                return std::io::read_to_string(child.stderr.take().unwrap()).unwrap();
            }
            assert!(
                Instant::now() < deadline,
                "service stayed alive after the storage failure"
            );
            thread::sleep(Duration::from_millis(20));
        }
    }
    pub fn isolated_command(&self, binary: &str) -> Command {
        isolated(self.root(), binary)
    }
    pub(crate) fn owner_key(&self) -> String {
        let key = format!(
            "{}{}",
            uuid::Uuid::new_v4().simple(),
            uuid::Uuid::new_v4().simple()
        );
        self.secrets.add(&key);
        key
    }
    pub(crate) fn bearer(&self, desktop: bool) -> String {
        format!(
            "Bearer {}",
            if desktop { &self.desktop } else { &self.native }
        )
    }
    pub(crate) fn raw_client(&self, header: &str, credential: &str) -> Client {
        Client::connect(self.port, header, credential, self.secrets.clone())
    }
    pub(crate) fn native(&self) -> Client {
        self.raw_client("Authorization", &self.bearer(false))
            .initialize()
    }
    pub(crate) fn desktop(&self) -> Client {
        self.raw_client("Authorization", &self.bearer(true))
            .initialize()
    }
    pub(crate) fn login_code(&self) -> String {
        let result = self.desktop().call("ui.loginLink", json!({}));
        result["url"]
            .as_str()
            .unwrap()
            .strip_prefix(&format!("{}/#login=", self.origin()))
            .expect("local login link")
            .to_owned()
    }
    pub fn exchange(&self, code: &str) -> (u16, Option<String>) {
        let host = format!("127.0.0.1:{}", self.port);
        let (status, cookie) = http(
            self.port,
            "POST /auth/exchange",
            &[
                ("Host", &host),
                ("Origin", &self.origin()),
                ("Content-Type", "application/json"),
            ],
            &json!({"code":code}).to_string(),
        );
        let cookie = cookie.map(|header| {
            let mut parts = header.split(';').map(str::trim);
            let pair = parts.next().unwrap();
            let attributes: Vec<_> = parts.collect();
            for expected in ["HttpOnly", "SameSite=Strict", "Path=/rpc"] {
                assert!(
                    attributes.contains(&expected),
                    "missing cookie attribute {expected}"
                );
            }
            let value = pair.strip_prefix("qo_session=").expect("session cookie");
            self.secrets.add(value);
            pair.to_owned()
        });
        (status, cookie)
    }
    pub(crate) fn cookie(&self) -> String {
        let (s, c) = self.exchange(&self.login_code());
        assert_eq!(s, 204);
        c.unwrap()
    }
    pub(crate) fn ui(&self) -> Client {
        self.raw_client("Cookie", &self.cookie()).initialize()
    }
}
