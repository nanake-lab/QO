use super::*;
use crate::backend::Outbound;
use std::time::Duration;

struct Factory(mpsc::Sender<EventSink>, mpsc::Sender<(String, Reply)>);
struct Process(mpsc::Sender<(String, Reply)>);

#[test]
fn an_overflowing_subscriber_can_resume_from_its_applied_cursor() {
    let root = tempfile::tempdir().unwrap();
    let settings = Settings::default();
    let service = ServiceBackend::new(
        Store::open_in(root.path(), &settings).unwrap(),
        &settings,
        root.path().join("workspaces"),
        None,
    )
    .unwrap();
    let peer = Peer {
        id: 1,
        role: Role::Browser,
    };
    let connect = |peer, capacity| {
        let (tx, rx) = tokio::sync::mpsc::channel(capacity);
        let (overflow, lagged) = tokio::sync::watch::channel(false);
        service.connected(peer, Notifications { tx, overflow });
        (rx, lagged)
    };
    let (_slow, lagged) = connect(peer, 1);
    service
        .call(peer, Method::EventsSubscribe, json!({"afterCursor":"0"}))
        .unwrap();
    let task = Uuid::new_v4().to_string();
    service
        .call(
            peer,
            Method::TaskCreate,
            json!({"taskId":task,"name":"overflow"}),
        )
        .unwrap();
    assert!(*lagged.borrow());
    service.disconnected(peer);
    let resumed = Peer { id: 2, ..peer };
    let (mut rx, lagged) = connect(resumed, 1);
    service
        .call(resumed, Method::EventsSubscribe, json!({"afterCursor":"0"}))
        .unwrap();
    let Outbound::Replay(mut replay) = rx.try_recv().unwrap() else {
        panic!("missing replay")
    };
    let page = replay.next_page().unwrap();
    assert_eq!(page.len(), 1);
    assert_eq!(page[0]["cursor"], "1");
    assert_eq!(page[0]["taskId"], task);
    assert!(!*lagged.borrow());
}
impl HarnessFactory for Factory {
    fn name(&self) -> &str {
        "test"
    }
    fn start(&self, _: &Task, _: &SessionOpenParams, sink: EventSink) -> io::Result<Started> {
        self.0.send(sink).unwrap();
        Ok(Started {
            process: Box::new(Process(self.1.clone())),
            version: "1".into(),
            model: None,
            reasoning_effort: None,
            service_tier: None,
        })
    }
}
impl Harness for Process {
    fn valid_decision(
        &self,
        _: &str,
        _: ApprovalKind,
        choice: Choice,
        answer: Option<&serde_json::Map<String, Value>>,
    ) -> bool {
        choice == Choice::Deny && answer.is_none()
    }
    fn start_turn(&mut self, _: &str, _: &str) -> io::Result<()> {
        Ok(())
    }
    fn append(&mut self, _: &str, _: &str) -> io::Result<()> {
        Ok(())
    }
    fn interrupt(&mut self, _: &str) -> io::Result<()> {
        Ok(())
    }
    fn finish(&mut self) -> io::Result<()> {
        Ok(())
    }
    fn terminate(&mut self) {}
    fn reply(&mut self, callback: &str, reply: Reply) -> io::Result<()> {
        self.0
            .send((callback.to_owned(), reply))
            .map_err(|_| io::Error::other("lost"))
    }
}

#[tokio::test]
async fn withdrawal_reveals_waiting_input_and_secret_is_delivered_only_to_its_callback() {
    let dir = tempfile::tempdir().unwrap();
    let settings = Settings::default();
    let (started, process) = mpsc::channel();
    let (replies, received) = mpsc::channel();
    let backend = ServiceBackend::new(
        Store::open_in(dir.path(), &settings).unwrap(),
        &settings,
        dir.path().join("workspaces"),
        Some(Box::new(Factory(started, replies))),
    )
    .unwrap();
    let owner = Peer {
        id: 1,
        role: Role::External,
    };
    let (tx, mut private) = tokio::sync::mpsc::channel(8);
    backend.connected(
        owner,
        Notifications {
            tx,
            overflow: tokio::sync::watch::channel(false).0,
        },
    );
    let task = Uuid::new_v4().to_string();
    let session = Uuid::new_v4().to_string();
    let turn = Uuid::new_v4().to_string();
    let owner_key = "ba".repeat(32);
    let call = |method, args| backend.call(owner, method, args).unwrap();
    call(
        Method::TaskCreate,
        json!({"taskId":task,"name":"test","ownerKey":owner_key}),
    );
    call(
        Method::SessionOpen,
        json!({"taskId":task,"sessionId":session}),
    );
    call(
        Method::TurnStart,
        json!({"sessionId":session,"turnId":turn,"input":"test"}),
    );
    let sink = process.recv().unwrap();
    let questions = serde_json::from_value(
        json!([{"id":"q","header":"note","question":"note?","allowOther":true,"secret":true}]),
    )
    .unwrap();
    sink.send(HarnessEvent::Input {
        callback: "input-id".into(),
        turn: turn.clone(),
        questions,
    })
    .unwrap();
    sink.send(HarnessEvent::Approval {
        callback: "approval-id".into(),
        turn: Some(turn.clone()),
        item: None,
        material: Material {
            kind: ApprovalKind::Permissions,
            raw: RawRequest {
                method: "private-method-marker".into(),
                params: Default::default(),
                context: None,
            },
            summary: "private-summary-marker".into(),
        },
    })
    .unwrap();
    async fn notice(rx: &mut tokio::sync::mpsc::Receiver<Outbound>, method: &str) -> Value {
        let Some(Outbound::Frames(frames)) =
            tokio::time::timeout(Duration::from_secs(5), rx.recv())
                .await
                .unwrap()
        else {
            panic!("missing notice")
        };
        assert_eq!(frames[0]["method"], method);
        frames[0]["params"].clone()
    }
    let input = notice(&mut private, "input.requested").await;
    let approval = notice(&mut private, "approval.requested").await;
    assert_eq!(
        call(Method::StateGet, json!({}))["sessions"][0]["state"],
        "waiting_approval"
    );
    sink.send(HarnessEvent::Withdrawn {
        callback: "approval-id".into(),
    })
    .unwrap();
    // This notification fences withdrawal on the same per-process channel without sleeps.
    sink.send(HarnessEvent::Input {
        callback: "second-input".into(),
        turn: turn.clone(),
        questions: serde_json::from_value(input["questions"].clone()).unwrap(),
    })
    .unwrap();
    let second = notice(&mut private, "input.requested").await;
    assert_eq!(
        call(
            Method::ApprovalGet,
            json!({"taskId":task,"approvalId":approval["approvalId"]})
        )["state"],
        "withdrawn"
    );
    assert_eq!(
        call(Method::StateGet, json!({}))["sessions"][0]["state"],
        "waiting_input"
    );
    let secret = "secret-input-marker";
    let answer = |request: &Value| json!({"sessionId":session,"turnId":turn,"requestId":request["requestId"],"answers":{"q":{"answers":[secret]}}});
    let mut invalid = answer(&input);
    invalid["answers"]["missing"] = json!({"answers":["invalid"]});
    assert_eq!(
        backend.call(owner, Method::InputAnswer, invalid),
        Err(ErrorReason::InvalidParams.into())
    );
    assert!(received.try_recv().is_err());
    for (request, callback) in [(&input, "input-id"), (&second, "second-input")] {
        call(Method::InputAnswer, answer(request));
        let (key, Reply::Input(answers)) = received.try_recv().unwrap() else {
            panic!("missing answers")
        };
        assert_eq!(key, callback);
        assert_eq!(answers["q"].answers, [secret]);
    }
    assert_eq!(
        call(Method::StateGet, json!({}))["sessions"][0]["state"],
        "running"
    );
    for file in std::fs::read_dir(dir.path()).unwrap() {
        let path = file.unwrap().path();
        if path.is_file() {
            let bytes = std::fs::read(path).unwrap();
            for marker in [
                secret,
                "private-method-marker",
                "private-summary-marker",
                &owner_key,
            ] {
                assert!(
                    !bytes
                        .windows(marker.len())
                        .any(|part| part == marker.as_bytes()),
                    "private data on disk"
                );
            }
        }
    }
}

struct SlowFactory(mpsc::Sender<()>, Mutex<mpsc::Receiver<()>>, Factory);
impl HarnessFactory for SlowFactory {
    fn name(&self) -> &str {
        "slow"
    }
    fn start(&self, t: &Task, a: &SessionOpenParams, sink: EventSink) -> io::Result<Started> {
        self.0.send(()).unwrap();
        self.1
            .lock()
            .unwrap()
            .recv_timeout(Duration::from_secs(10))
            .unwrap();
        self.2.start(t, a, sink)
    }
}
#[test]
fn slow_start_releases_writer_and_live_lock() {
    let dir = tempfile::tempdir().unwrap();
    let settings = Settings::default();
    let (entered, waiting) = mpsc::channel();
    let (release, gate) = mpsc::channel();
    let (events, _sink) = mpsc::channel();
    let factory = SlowFactory(
        entered,
        Mutex::new(gate),
        Factory(events, mpsc::channel().0),
    );
    let backend = ServiceBackend::new(
        Store::open_in(dir.path(), &settings).unwrap(),
        &settings,
        dir.path().into(),
        Some(Box::new(factory)),
    )
    .unwrap();
    let peer = Peer {
        id: 1,
        role: Role::Browser,
    };
    let other = Peer { id: 2, ..peer };
    let task = Uuid::new_v4().to_string();
    let sid = Uuid::new_v4().to_string();
    backend
        .call(
            peer,
            Method::TaskCreate,
            json!({"taskId":task,"name":"slow"}),
        )
        .unwrap();
    let args = json!({"taskId":task,"sessionId":sid});
    let worker = backend.clone();
    let open = args.clone();
    let startup = thread::spawn(move || worker.call(peer, Method::SessionOpen, open).unwrap());
    waiting.recv_timeout(Duration::from_secs(5)).unwrap();
    let (done, verified) = mpsc::channel();
    let worker = backend.clone();
    let cancel = args.clone();
    let observer = thread::spawn(move || {
        assert_eq!(
            worker.call(other, Method::StateGet, json!({})).unwrap()["sessions"][0]["state"],
            "starting"
        );
        worker
            .call(
                other,
                Method::TaskCreate,
                json!({"taskId":Uuid::new_v4().to_string(),"name":"concurrent write"}),
            )
            .unwrap();
        let cancelled = worker.call(other, Method::SessionCancel, cancel).unwrap();
        done.send(cancelled).unwrap();
    });
    let cancelled = verified.recv_timeout(Duration::from_secs(3));
    release.send(()).unwrap();
    assert_eq!(startup.join().unwrap(), cancelled.unwrap());
    observer.join().unwrap();
    assert_eq!(
        backend.call(peer, Method::SessionOpen, args).unwrap()["state"],
        "cancelled"
    );
    assert!(backend.live.lock().unwrap().processes.is_empty());
}

#[test]
#[ignore = "local SQLite throughput measurement; requires node on PATH"]
fn command_output_measurement() {
    use std::{
        io::Read,
        process::{Command, Stdio},
        time::Instant,
    };
    let dir = tempfile::tempdir().unwrap();
    let mut settings = Settings::default();
    if let Ok(bytes) = std::env::var("QO_BENCH_BATCH_BYTES") {
        settings.output_batch_bytes = bytes.parse().unwrap();
    }
    let (tx, rx) = mpsc::channel();
    let backend = ServiceBackend::new(
        Store::open_in(dir.path(), &settings).unwrap(),
        &settings,
        dir.path().into(),
        Some(Box::new(Factory(tx, mpsc::channel().0))),
    )
    .unwrap();
    let peer = Peer {
        id: 1,
        role: Role::Browser,
    };
    let uuid = || Uuid::new_v4().to_string();
    let (task, sid, turn, item) = (uuid(), uuid(), uuid(), uuid());
    backend
        .call(
            peer,
            Method::TaskCreate,
            json!({"taskId":task,"name":"output"}),
        )
        .unwrap();
    backend
        .call(
            peer,
            Method::SessionOpen,
            json!({"taskId":task,"sessionId":sid}),
        )
        .unwrap();
    backend
        .call(
            peer,
            Method::TurnStart,
            json!({"sessionId":sid,"turnId":turn,"input":"output"}),
        )
        .unwrap();
    let sink = rx.recv().unwrap();
    sink.send(HarnessEvent::Item(decode(json!({"kind":"command_execution","turnId":turn,"itemId":item,"state":"running","startedAt":now(),"content":{"command":"node output","output":""}})).unwrap())).unwrap();
    let started = Instant::now();
    let mut child = Command::new("node")
        .args([
            "-e",
            "process.stdout.write('line of output!\\n'.repeat(262144))",
        ])
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdout = child.stdout.take().unwrap();
    let mut bytes = [0; 4096];
    let mut count = 0;
    loop {
        let n = stdout.read(&mut bytes).unwrap();
        if n == 0 {
            break;
        }
        count += n;
        sink.send(HarnessEvent::Delta {
            turn: turn.clone(),
            delta: ItemDelta {
                item_id: item.clone(),
                delta: std::str::from_utf8(&bytes[..n]).unwrap().into(),
            },
        })
        .unwrap();
    }
    assert!(child.wait().unwrap().success());
    assert_eq!(count, 4 * 1024 * 1024);
    sink.send(HarnessEvent::TurnEnded {
        turn: turn.clone(),
        state: TurnState::Completed,
        error: None,
    })
    .unwrap();
    loop {
        let done = backend
            .store
            .read(|v| Ok(object::<Turn>(v, Projection::Turn, &turn)?.state == TurnState::Completed))
            .unwrap();
        if done {
            break;
        }
        assert!(started.elapsed() < Duration::from_secs(600));
        thread::sleep(Duration::from_millis(10));
    }
    let elapsed = started.elapsed();
    let events = backend
        .store
        .read(|v| {
            let body: Value = object(v, Projection::Item, &item)?;
            assert_eq!(
                body["content"]["output"].as_str().unwrap(),
                "line of output!\n".repeat(262144)
            );
            Ok(v.db.query_row(
                "SELECT count(*) FROM events WHERE kind='item.delta'",
                [],
                |r| r.get::<_, u64>(0),
            )?)
        })
        .unwrap();
    println!("{count} bytes: {elapsed:?}, {events} public deltas");
}

#[test]
fn split_credentials_are_filtered_in_live_deltas_replay_and_snapshot() {
    let dir = tempfile::tempdir().unwrap();
    let settings = Settings::default();
    let (tx, rx) = mpsc::channel();
    let backend = ServiceBackend::new(
        Store::open_in(dir.path(), &settings).unwrap(),
        &settings,
        dir.path().into(),
        Some(Box::new(Factory(tx, mpsc::channel().0))),
    )
    .unwrap();
    let peer = Peer {
        id: 1,
        role: Role::Browser,
    };
    let uuid = || Uuid::new_v4().to_string();
    let (task, sid, turn, item) = (uuid(), uuid(), uuid(), uuid());
    let secret = "private split credential";
    backend.remember_secret(secret);
    backend
        .call(
            peer,
            Method::TaskCreate,
            json!({"taskId":task,"name":"filter"}),
        )
        .unwrap();
    backend
        .call(
            peer,
            Method::SessionOpen,
            json!({"taskId":task,"sessionId":sid}),
        )
        .unwrap();
    backend
        .call(
            peer,
            Method::TurnStart,
            json!({"sessionId":sid,"turnId":turn,"input":secret}),
        )
        .unwrap();
    let sink = rx.recv().unwrap();
    sink.send(HarnessEvent::Item(decode(json!({"kind":"command_execution","turnId":turn,"itemId":item,"state":"running","startedAt":now(),"content":{"command":secret,"output":""}})).unwrap())).unwrap();
    let prefix = "safe ".repeat(40);
    sink.send(HarnessEvent::Delta {
        turn: turn.clone(),
        delta: ItemDelta {
            item_id: item.clone(),
            delta: format!("{prefix}private split"),
        },
    })
    .unwrap();
    // A different notification forces the two chunks into distinct write batches.
    sink.send(HarnessEvent::Rerouted {
        model: "rerouted".into(),
    })
    .unwrap();
    sink.send(HarnessEvent::Delta {
        turn: turn.clone(),
        delta: ItemDelta {
            item_id: item.clone(),
            delta: " credential done".into(),
        },
    })
    .unwrap();
    sink.send(HarnessEvent::TurnEnded {
        turn: turn.clone(),
        state: TurnState::Completed,
        error: None,
    })
    .unwrap();
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    loop {
        let state = backend.call(peer, Method::StateGet, json!({})).unwrap();
        if state["turns"][0]["state"] == "completed" {
            assert_eq!(state["turns"][0]["input"], "[redacted]");
            assert_eq!(
                state["items"][0]["content"]["output"],
                format!("{prefix}[redacted] done")
            );
            assert_eq!(state["items"][0]["content"]["command"], "[redacted]");
            break;
        }
        assert!(std::time::Instant::now() < deadline);
        thread::yield_now();
    }
    let mut replay = backend.store.begin_replay(0).unwrap();
    let mut output = String::new();
    loop {
        let page = replay.next_page().unwrap();
        if page.is_empty() {
            break;
        }
        for e in page {
            assert!(!e.to_string().contains("private split"));
            if e["kind"] == "item.delta" {
                output.push_str(e["payload"]["delta"].as_str().unwrap());
            }
        }
    }
    assert_eq!(output, format!("{prefix}[redacted] done"));
}

#[test]
#[ignore = "uses the locally installed pinned Codex; no model turn"]
fn real_codex_handshake_and_finish() {
    let dir = tempfile::tempdir().unwrap();
    let settings = Settings::default();
    let backend = ServiceBackend::new(
        Store::open_in(dir.path(), &settings).unwrap(),
        &settings,
        dir.path().into(),
        Some(Box::new(settings.codex_factory(dir.path()))),
    )
    .unwrap();
    let peer = Peer {
        id: 1,
        role: Role::Browser,
    };
    let (task, sid) = (Uuid::new_v4().to_string(), Uuid::new_v4().to_string());
    let work = dir.path().join("work");
    std::fs::create_dir(&work).unwrap();
    backend
        .call(
            peer,
            Method::TaskCreate,
            json!({"taskId":task,"name":"handshake","cwd":work}),
        )
        .unwrap();
    let args = json!({"taskId":task,"sessionId":sid});
    let session = backend
        .call(peer, Method::SessionOpen, args.clone())
        .unwrap();
    assert_eq!(session["state"], "idle", "{}", session["error"]);
    assert_eq!(session["harnessVersion"], "0.162.1");
    assert!(dir.path().join("codex").is_dir());
    assert_eq!(
        backend.call(peer, Method::SessionFinish, args).unwrap()["state"],
        "completed"
    );
}

#[test]
fn configured_cli_version_mismatch_is_a_failed_session_with_both_versions() {
    let dir = tempfile::tempdir().unwrap();
    let settings = Settings {
        codex_path: "node".into(),
        ..Default::default()
    };
    let backend = ServiceBackend::new(
        Store::open_in(dir.path(), &settings).unwrap(),
        &settings,
        dir.path().into(),
        Some(Box::new(settings.codex_factory(dir.path()))),
    )
    .unwrap();
    let peer = Peer {
        id: 1,
        role: Role::Browser,
    };
    let (task, sid) = (Uuid::new_v4().to_string(), Uuid::new_v4().to_string());
    backend
        .call(
            peer,
            Method::TaskCreate,
            json!({"taskId":task,"name":"wrong version","cwd":dir.path()}),
        )
        .unwrap();
    let result = backend
        .call(
            peer,
            Method::SessionOpen,
            json!({"taskId":task,"sessionId":sid}),
        )
        .unwrap();
    assert_eq!(result["state"], "failed");
    let node = std::process::Command::new("node")
        .arg("--version")
        .output()
        .unwrap();
    let actual = String::from_utf8(node.stdout).unwrap();
    assert_eq!(
        result["error"],
        format!(
            "Codex version mismatch: expected 0.162.1, actual {}",
            actual.trim()
        )
    );
}
