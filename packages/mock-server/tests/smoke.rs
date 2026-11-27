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
                assert!(
                    qo_protocol::validate_output(name, &v["result"]),
                    "{name} failed: {:?}",
                    v["error"]
                );
                return v["result"].clone();
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
#[test]
fn complete_approval_and_reconnect_replay() {
    let dir = tempfile::tempdir().unwrap();
    let child = Command::new(env!("CARGO_BIN_EXE_qo-mock"))
        .args(["--port", "0", "--root"])
        .arg(dir.path())
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
    let mut owner = Client::new(dir.path());
    let mut observer = Client::new(dir.path());
    let task = uuid::Uuid::new_v4().to_string();
    owner.call(
        "task.create",
        json!({"taskId":task,"name":"Smoke","ownerKey":"ab".repeat(32)}),
    );
    let sid = owner.call("session.open", json!({"taskId":task}))["sessionId"].clone();
    let snapshot = observer.call("state.get", json!({}));
    let after = snapshot["cursor"].as_str().unwrap().parse::<u64>().unwrap();
    observer.call("events.subscribe", json!({"afterCursor":after.to_string()}));
    owner.call(
        "turn.start",
        json!({"sessionId":sid,"input":"scenario:complete"}),
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
        json!({"sessionId":sid,"input":"scenario:approval"}),
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
