use serde_json::{Value, json};
use std::{
    collections::VecDeque,
    fs,
    io::{BufRead, BufReader},
    net::TcpStream,
    path::Path,
    process::{Child, Command, Stdio},
    time::Duration,
};
use tungstenite::{Message, WebSocket, client::IntoClientRequest};

fn new_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

struct Process(Child);
impl Drop for Process {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

struct Client {
    socket: WebSocket<TcpStream>,
    notifications: VecDeque<Value>,
    seq: u64,
}
impl Client {
    fn new(root: &Path) -> Self {
        let endpoint: Value =
            serde_json::from_slice(&fs::read(root.join("run/endpoint.json")).unwrap()).unwrap();
        let address = format!("127.0.0.1:{}", endpoint["port"]);
        let mut request = format!("ws://{address}/rpc").into_client_request().unwrap();
        request
            .headers_mut()
            .insert("Origin", format!("http://{address}").parse().unwrap());
        request.headers_mut().insert(
            "Authorization",
            format!("Bearer {}", endpoint["token"].as_str().unwrap())
                .parse()
                .unwrap(),
        );
        let stream = TcpStream::connect(address).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(10)))
            .unwrap();
        let socket = tungstenite::client(request, stream)
            .map_err(|_| "mock refused upgrade")
            .unwrap()
            .0;
        let mut c = Self {
            socket,
            notifications: VecDeque::new(),
            seq: 0,
        };
        c.call("initialize", json!({"versions":["qo.v1"]}));
        c
    }
    fn read(&mut self) -> Value {
        serde_json::from_str(self.socket.read().unwrap().to_text().unwrap()).unwrap()
    }
    fn call(&mut self, name: &str, params: Value) -> Value {
        let reply = self.rpc(name, params);
        assert!(
            qo_protocol::validate_output(name, &reply["result"]),
            "{name}: {:?}",
            reply["error"]
        );
        reply["result"].clone()
    }
    fn rejected(&mut self, name: &str, params: Value, reason: &str) {
        let reply = self.rpc(name, params);
        assert!(qo_protocol::validate_error(&reply["error"]));
        assert_eq!(reply["error"]["data"]["reason"], reason, "{name}");
    }
    fn rpc(&mut self, name: &str, params: Value) -> Value {
        self.seq += 1;
        self.socket
            .send(Message::Text(
                json!({"jsonrpc":"2.0","id":self.seq,"method":name,"params":params})
                    .to_string()
                    .into(),
            ))
            .unwrap();
        loop {
            let v = self.read();
            if v.get("id").is_some() {
                assert_eq!(v["id"], self.seq);
                return v;
            }
            self.notifications.push_back(v);
        }
    }
    fn next(&mut self) -> Value {
        let v = self
            .notifications
            .pop_front()
            .unwrap_or_else(|| self.read());
        assert!(qo_protocol::validate_output(
            v["method"].as_str().unwrap(),
            &v["params"]
        ));
        v
    }
}
fn start_mock(root: &Path) -> Process {
    start_mock_with(root, &[])
}
fn start_mock_with(root: &Path, options: &[&str]) -> Process {
    let child = Command::new(env!("CARGO_BIN_EXE_qo-mock"))
        .args(["--port", "0", "--root"])
        .arg(root)
        .args(options)
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut server = Process(child);
    let mut lines = BufReader::new(server.0.stdout.take().unwrap()).lines();
    let address = lines.next().unwrap().unwrap();
    assert!(address.starts_with("http://127.0.0.1:"));
    assert!(Path::new(&lines.next().unwrap().unwrap()).is_absolute());
    assert!(
        lines
            .next()
            .unwrap()
            .unwrap()
            .starts_with(&format!("{address}/#login="))
    );
    server
}

#[test]
fn complete_approval_and_reconnect_replay() {
    let dir = tempfile::tempdir().unwrap();
    let _server = start_mock(dir.path());
    let mut owner = Client::new(dir.path());
    let mut observer = Client::new(dir.path());
    let task = uuid::Uuid::new_v4().to_string();
    owner.call(
        "task.create",
        json!({"taskId":task,"name":"Smoke","ownerKey":"ab".repeat(32)}),
    );
    let sid = owner.call("session.open", json!({"taskId":task,"sessionId":new_id()}))["sessionId"]
        .clone();
    let snapshot = observer.call("state.get", json!({}));
    let after = snapshot["cursor"].as_str().unwrap().parse::<u64>().unwrap();
    observer.call("events.subscribe", json!({"afterCursor":after.to_string()}));
    owner.call(
        "turn.start",
        json!({"sessionId":sid,"turnId":new_id(),"input":"scenario:complete"}),
    );
    let mut cursor = after;
    let mut text = String::new();
    let mut command_output = String::new();
    let mut assistant_item = Value::Null;
    let mut command = false;
    let mut usage = false;
    loop {
        let v = observer.next();
        assert_eq!(v["method"], "event");
        let event = &v["params"];
        cursor += 1;
        assert_eq!(event["cursor"], cursor.to_string());
        match event["kind"].as_str().unwrap() {
            "item.changed" if event["payload"]["kind"] == "assistant_message" => {
                assistant_item = event["payload"]["itemId"].clone();
            }
            "item.delta" => {
                let delta = event["payload"]["delta"].as_str().unwrap();
                if event["payload"]["itemId"] == assistant_item {
                    text.push_str(delta);
                } else {
                    command_output.push_str(delta);
                }
            }
            "item.changed" if event["payload"]["kind"] == "command_execution" => {
                command = event["payload"]["content"]["exitCode"] == 0
            }
            "usage.updated" => usage = true,
            "session.changed" if event["payload"]["state"] == "idle" => break,
            _ => {}
        }
    }
    assert!(command && usage);
    assert_eq!(text, "收到。 正在检查示例。");
    assert_eq!(command_output, "demo\n");
    drop(observer);
    owner.call(
        "turn.start",
        json!({"sessionId":sid,"turnId":new_id(),"input":"scenario:approval"}),
    );
    let n = owner.next();
    assert_eq!(n["method"], "approval.requested");
    let a = n["params"].clone();
    owner.call(
        "approval.decide",
        json!({"taskId":task,"approvalId":a["approvalId"],"digest":a["digest"],
        "decisionId":uuid::Uuid::new_v4().to_string(),"choice":"allow_once"}),
    );
    let mut resumed = Client::new(dir.path());
    resumed.call(
        "events.subscribe",
        json!({"afterCursor":cursor.to_string()}),
    );
    let mut executed = 0;
    loop {
        let n = resumed.next();
        assert_eq!(n["method"], "event");
        let e = &n["params"];
        cursor += 1;
        assert_eq!(e["cursor"], cursor.to_string());
        if e["kind"] == "item.changed" && e["payload"]["content"]["output"] == "approved\n" {
            executed += 1;
        }
        if e["kind"] == "session.changed" && e["payload"]["state"] == "idle" {
            break;
        }
    }
    assert_eq!(executed, 1);
    assert_eq!(
        resumed.call("state.get", json!({}))["cursor"],
        cursor.to_string()
    );
}

/// Collect only public events, including the transition that ends this interval.
fn until_session_state(observer: &mut Client, state: &str) -> Vec<Value> {
    let mut events = Vec::new();
    loop {
        let message = observer.next();
        assert_eq!(message["method"], "event");
        let event = &message["params"];
        events.push(event.clone());
        if event["kind"] == "session.changed" && event["payload"]["state"] == state {
            return events;
        }
    }
}

#[test]
fn observers_follow_the_same_item_before_and_after_approval() {
    let dir = tempfile::tempdir().unwrap();
    let _server = start_mock(dir.path());
    let mut owner = Client::new(dir.path());
    let task = uuid::Uuid::new_v4().to_string();
    owner.call(
        "task.create",
        json!({"taskId":task,"name":"Visibility","ownerKey":"ab".repeat(32)}),
    );

    for (scenario, action) in [
        ("approval", "allow_once"),
        ("approval", "deny"),
        ("file-approval", "allow_once"),
        ("file-approval", "deny"),
        ("approval", "cancel"),
    ] {
        let command = scenario == "approval";
        let mut content = if command {
            json!({"command": "echo approved", "output": ""})
        } else {
            json!({"path": "demo.txt", "diff": "+hello\n"})
        };
        let session =
            owner.call("session.open", json!({"taskId":task,"sessionId":new_id()}))["sessionId"]
                .clone();
        let mut observer = Client::new(dir.path());
        let cursor = observer.call("state.get", json!({}))["cursor"].clone();
        observer.call("events.subscribe", json!({"afterCursor":cursor}));
        owner.call(
            "turn.start",
            json!({"sessionId":session,"turnId":new_id(),"input":format!("scenario:{scenario}")}),
        );
        let notice = owner.next();
        assert_eq!(notice["method"], "approval.requested");
        let request = &notice["params"];
        assert!(request["raw"].is_object());
        let typed = serde_json::from_value(request.clone()).unwrap();
        assert_eq!(qo_protocol::approval_digest(&typed), request["digest"]);
        let lookup = json!({"taskId": task, "approvalId": request["approvalId"]});
        let pending = owner.call("approval.get", lookup.clone());
        assert_eq!(pending["state"], "pending");
        assert_eq!(pending["digest"], request["digest"]);
        assert!(pending.get("decision").is_none());
        observer.rejected("approval.get", lookup.clone(), "forbidden");

        let before = until_session_state(&mut observer, "waiting_approval");
        let starts: Vec<_> = before
            .iter()
            .filter(|e| e["kind"] == "item.changed")
            .collect();
        assert_eq!(starts.len(), 1);
        let start = &starts[0]["payload"];
        assert_eq!(start["itemId"], request["itemId"]);
        assert_eq!(start["state"], "running");
        assert_eq!(start["content"], content);
        assert!(start.get("endedAt").is_none());
        assert!(!before.iter().any(|e| e["kind"] == "item.delta"));
        let snapshot = observer.call("state.get", json!({}));
        assert!(snapshot["items"].as_array().unwrap().contains(start));
        assert!(
            !snapshot
                .to_string()
                .contains(request["digest"].as_str().unwrap())
        );

        let terminal_session = if action == "cancel" {
            owner.call("session.cancel", json!({"taskId":task,"sessionId":session}));
            "cancelled"
        } else {
            let reply = owner.call("approval.decide", json!({
                "taskId": task, "approvalId": request["approvalId"], "digest": request["digest"],
                "decisionId": uuid::Uuid::new_v4().to_string(), "choice": action
            }));
            assert_eq!(reply["delivery"], "sent");
            let status = owner.call("approval.get", lookup.clone());
            assert_eq!(status["state"], "decided");
            for key in ["decisionId", "decidedAt", "delivery", "choice"] {
                assert_eq!(status["decision"][key], reply[key]);
            }
            assert!(status.get("raw").is_none() && status.get("summary").is_none());
            assert!(status["decision"].get("answer").is_none());
            "idle"
        };
        if action == "cancel" {
            assert_eq!(owner.call("approval.get", lookup)["state"], "withdrawn");
        }
        let after = until_session_state(&mut observer, terminal_session);
        let updates: Vec<_> = after
            .iter()
            .filter(|e| e["kind"] == "item.changed")
            .collect();
        assert_eq!(updates.len(), 1);
        let end = &updates[0]["payload"];
        assert_eq!(end["itemId"], start["itemId"]);
        assert_eq!(end["startedAt"], start["startedAt"]);
        assert!(end["endedAt"].is_string());
        let allowed = action == "allow_once";
        assert_eq!(
            end["state"],
            if allowed { "completed" } else { "interrupted" }
        );
        let deltas: Vec<_> = after.iter().filter(|e| e["kind"] == "item.delta").collect();
        if allowed && command {
            assert_eq!(deltas.len(), 1);
            assert_eq!(
                deltas[0]["payload"],
                json!({"itemId":start["itemId"],"delta":"approved\n"})
            );
            content["output"] = json!("approved\n");
            content["exitCode"] = json!(0);
        } else {
            assert!(deltas.is_empty());
        }
        assert_eq!(end["content"], content);
    }
}

#[test]
fn retries_return_first_results_even_after_close() {
    let dir = tempfile::tempdir().unwrap();
    let _server = start_mock(dir.path());
    let mut owner = Client::new(dir.path());
    let mut observer = Client::new(dir.path());
    let task = new_id();
    let session = new_id();
    let turn = new_id();
    let create = json!({"taskId":task,"name":"Retry","ownerKey":"ab".repeat(32)});
    let open = json!({"taskId":task,"sessionId":session,"model":"initial"});
    let start = json!({"sessionId":session,"turnId":turn,"input":"scenario:approval"});
    let append =
        json!({"sessionId":session,"turnId":turn,"appendId":new_id(),"input":"one addition"});
    let created = owner.call("task.create", create.clone());
    let opened = owner.call("session.open", open.clone());
    assert_eq!(opened["harnessVersion"], env!("CARGO_PKG_VERSION"));
    let started = owner.call("turn.start", start.clone());
    let request = owner.next();
    assert_eq!(request["method"], "approval.requested");
    let appended = owner.call("turn.append", append.clone());
    let writes = [
        ("task.create", create, created, "name"),
        ("session.open", open, opened, "model"),
        ("turn.start", start, started, "input"),
        ("turn.append", append, appended, "input"),
    ];
    let snapshot = owner.call("state.get", json!({}));
    for (method, params, first, field) in &writes {
        assert_eq!(owner.call(method, params.clone()), *first);
        let mut changed = params.clone();
        changed[*field] = json!("changed");
        owner.rejected(method, changed, "conflict");
        observer.rejected(method, params.clone(), "forbidden");
    }
    owner.rejected(
        "session.open",
        json!({"taskId":task,"sessionId":turn}),
        "conflict",
    );
    assert_eq!(owner.call("state.get", json!({})), snapshot);

    owner.call("session.cancel", json!({"taskId":task,"sessionId":session}));
    owner.call("task.close", json!({"taskId":task}));
    drop(owner);
    let mut owner = Client::new(dir.path());
    owner.call(
        "task.attach",
        json!({"taskId":task,"ownerKey":"ab".repeat(32)}),
    );
    let closed = owner.call("state.get", json!({}));
    for (method, params, first, _) in writes {
        assert_eq!(owner.call(method, params), first);
    }
    assert_eq!(owner.call("state.get", json!({})), closed);
    let status = owner.call(
        "approval.get",
        json!({"taskId":task,"approvalId":request["params"]["approvalId"]}),
    );
    assert_eq!(status["state"], "withdrawn");
}

#[test]
fn reroute_is_visible_on_the_original_subscription() {
    let dir = tempfile::tempdir().unwrap();
    let _server = start_mock(dir.path());
    let mut c = Client::new(dir.path());
    let task = new_id();
    let session = new_id();
    c.call(
        "task.create",
        json!({"taskId":task,"ownerKey":"ab".repeat(32),"name":"Route"}),
    );
    c.call(
        "session.open",
        json!({"taskId":task,"sessionId":session,"model":"initial"}),
    );
    let snapshot = c.call("state.get", json!({}));
    let mut cursor: u64 = snapshot["cursor"].as_str().unwrap().parse().unwrap();
    c.call(
        "events.subscribe",
        json!({"afterCursor":snapshot["cursor"]}),
    );
    c.rejected("events.subscribe", json!({"afterCursor":"0"}), "conflict");
    c.call(
        "turn.start",
        json!({"sessionId":session,"turnId":new_id(),"input":"scenario:reroute"}),
    );
    let mut text_seen = false;
    let mut rerouted = false;
    for e in until_session_state(&mut c, "idle") {
        cursor += 1;
        assert_eq!(e["cursor"], cursor.to_string());
        if e["kind"] == "item.delta" {
            text_seen = true;
        }
        if e["kind"] == "session.changed"
            && e["payload"]["state"] == "running"
            && e["payload"]["model"] == "mock-rerouted"
        {
            assert!(text_seen);
            rerouted = true;
        }
    }
    assert!(rerouted);
    let snapshot = c.call("state.get", json!({}));
    assert_eq!(snapshot["sessions"][0]["model"], "mock-rerouted");
    assert_eq!(snapshot["cursor"], cursor.to_string());
}

#[test]
fn expired_approval_remains_queryable_after_renewal_and_close() {
    let dir = tempfile::tempdir().unwrap();
    let _server = start_mock_with(dir.path(), &["--approval-ttl-seconds", "0"]);
    let mut owner = Client::new(dir.path());
    let task = new_id();
    let session = new_id();
    owner.call(
        "task.create",
        json!({"taskId":task,"name":"Expiry","ownerKey":"ab".repeat(32)}),
    );
    owner.call("session.open", json!({"taskId":task,"sessionId":session}));
    owner.call(
        "turn.start",
        json!({"sessionId":session,"turnId":new_id(),"input":"scenario:approval"}),
    );
    let request = owner.next();
    let lookup = json!({"taskId":task,"approvalId":request["params"]["approvalId"]});
    let expired = owner.call("approval.get", lookup.clone());
    assert_eq!(expired["state"], "expired");
    assert!(expired.get("decision").is_none());
    let renewed = owner.call("approval.renew", lookup.clone());
    assert_ne!(renewed["approvalId"], expired["approvalId"]);
    owner.call("session.cancel", json!({"taskId":task,"sessionId":session}));
    owner.call("task.close", json!({"taskId":task}));
    assert_eq!(owner.call("approval.get", lookup), expired);
    let replacement = owner.call(
        "approval.get",
        json!({"taskId":task,"approvalId":renewed["approvalId"]}),
    );
    assert_eq!(replacement["state"], "withdrawn");
}
