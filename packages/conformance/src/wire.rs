use qo_protocol::{
    ApprovalRequest, approval_digest, notification_schema, validate_error, validate_output,
    validate_params,
};
use serde_json::{Value, json};
use std::{
    cell::RefCell,
    collections::VecDeque,
    io::{BufRead, BufReader, ErrorKind, Write},
    net::TcpStream,
    rc::Rc,
    time::{Duration, Instant},
};
use tungstenite::{Error, Message, WebSocket, client::IntoClientRequest, error::ProtocolError};

#[derive(Default, Clone)]
pub(crate) struct Secrets(Rc<RefCell<Vec<String>>>);
impl Secrets {
    pub fn add(&self, value: &str) {
        assert!(!value.is_empty(), "empty credential");
        self.0.borrow_mut().push(value.to_owned());
    }
    fn check(&self, text: &str) {
        assert!(
            !self.0.borrow().iter().any(|s| text.contains(s)),
            "credential in incoming frame"
        );
    }
}

fn tcp(port: u16) -> TcpStream {
    let stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    // Bound blocked reads and writes, including an incorrectly accepted HTTP upgrade.
    let timeout = Some(Duration::from_secs(8));
    stream.set_read_timeout(timeout).unwrap();
    stream.set_write_timeout(timeout).unwrap();
    stream
}

pub(crate) struct Client {
    socket: WebSocket<TcpStream>,
    secrets: Secrets,
    seq: u64,
    notices: VecDeque<Value>,
}
impl Client {
    pub fn connect(port: u16, header: &str, credential: &str, secrets: Secrets) -> Self {
        let mut req = format!("ws://127.0.0.1:{port}/rpc")
            .into_client_request()
            .unwrap();
        req.headers_mut().insert(
            "Origin",
            format!("http://127.0.0.1:{port}").parse().unwrap(),
        );
        req.headers_mut().insert(
            tungstenite::http::HeaderName::from_bytes(header.as_bytes()).unwrap(),
            credential.parse().unwrap(),
        );
        let (socket, _) = tungstenite::client(req, tcp(port))
            .unwrap_or_else(|_| panic!("WebSocket upgrade failed"));
        Self {
            socket,
            secrets,
            seq: 0,
            notices: VecDeque::new(),
        }
    }
    pub fn initialize(mut self) -> Self {
        assert_eq!(
            self.call("initialize", json!({"versions":["qo.v1"]})),
            json!({"version":"qo.v1"})
        );
        self
    }
    fn frame(&mut self) -> Value {
        let msg = self.socket.read().expect("expected RPC frame");
        let text = msg.to_text().expect("expected JSON text");
        self.secrets.check(text);
        assert!(msg.is_text(), "expected text frame");
        let value: Value = serde_json::from_str(text).expect("malformed JSON");
        self.secrets.check(&value.to_string()); // Also catch credentials escaped on the wire.
        assert_eq!(value["jsonrpc"], "2.0");
        value
    }
    fn check_notice(&self, n: &Value) {
        let name = n["method"].as_str().expect("notification method");
        assert!(n.get("id").is_none() && n.get("result").is_none() && n.get("error").is_none());
        assert!(
            notification_schema(name).is_some() && validate_output(name, &n["params"]),
            "invalid notification {name}"
        );
        if name == "approval.requested" {
            let a: ApprovalRequest = serde_json::from_value(n["params"].clone()).unwrap();
            assert_eq!(approval_digest(&a), a.digest, "approval digest");
        }
    }
    fn request(&mut self, name: &str, params: Value) -> Result<Value, Value> {
        assert!(
            validate_params(name, &params),
            "invalid test input for {name}"
        );
        self.seq += 1;
        self.socket
            .send(Message::Text(
                json!({"jsonrpc":"2.0","id":self.seq,"method":name,"params":params})
                    .to_string()
                    .into(),
            ))
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(12);
        loop {
            assert!(Instant::now() < deadline, "no reply to {name}");
            let v = self.frame();
            if v.get("id").is_none() {
                self.check_notice(&v);
                self.notices.push_back(v);
                continue;
            }
            assert_eq!(v["id"], self.seq);
            assert!(v.get("method").is_none());
            match (v.get("result"), v.get("error")) {
                (Some(r), None) => {
                    assert!(validate_output(name, r), "invalid result for {name}");
                    return Ok(r.clone());
                }
                (None, Some(e)) => {
                    assert!(validate_error(e), "invalid error for {name}");
                    return Err(e.clone());
                }
                _ => panic!("invalid response envelope for {name}"),
            }
        }
    }
    pub fn call(&mut self, name: &str, p: Value) -> Value {
        self.request(name, p)
            .unwrap_or_else(|e| panic!("{name} failed: {}", e["data"]["reason"]))
    }
    pub fn reject(&mut self, name: &str, p: Value, reason: &str) -> Value {
        let Err(e) = self.request(name, p) else {
            panic!("expected {name} to be rejected")
        };
        assert_eq!(e["data"]["reason"], reason, "{name}");
        e["data"].clone()
    }
    pub fn notice(&mut self, name: &str) -> Value {
        let n = match self.notices.pop_front() {
            Some(n) => n,
            None => {
                let n = self.frame();
                self.check_notice(&n);
                n
            }
        };
        assert_eq!(n["method"], name);
        n["params"].clone()
    }
    pub fn subscribe(&mut self, cursor: u64) {
        self.call(
            "events.subscribe",
            json!({"afterCursor":cursor.to_string()}),
        );
    }
    pub fn state(&mut self) -> Value {
        self.call("state.get", json!({}))
    }
    pub fn events_until(&mut self, mut predicate: impl FnMut(&Value) -> bool) -> Vec<Value> {
        let mut events = Vec::new();
        let deadline = Instant::now() + Duration::from_secs(12);
        loop {
            assert!(Instant::now() < deadline, "missing public event");
            let event = self.notice("event");
            let done = predicate(&event);
            events.push(event);
            if done {
                return events;
            }
        }
    }
    pub fn expect_closed(&mut self) {
        match self.socket.read() {
            Ok(Message::Close(frame)) => {
                if let Some(f) = frame {
                    self.secrets.check(&f.reason);
                }
            }
            Err(
                Error::ConnectionClosed
                | Error::AlreadyClosed
                | Error::Protocol(ProtocolError::ResetWithoutClosingHandshake),
            ) => {}
            Err(Error::Io(e))
                if matches!(
                    e.kind(),
                    ErrorKind::UnexpectedEof
                        | ErrorKind::ConnectionReset
                        | ErrorKind::ConnectionAborted
                ) => {}
            Ok(Message::Text(t)) => {
                self.secrets.check(&t);
                panic!("unexpected frame after rejected handshake")
            }
            _ => panic!("expected peer to close without further frames"),
        }
    }
}

// Keep repeated headers intact and return the actual status even for an unexpected 101.
pub(crate) fn http(
    port: u16,
    line: &str,
    headers: &[(&str, &str)],
    body: &str,
) -> (u16, Option<String>) {
    let mut stream = tcp(port);
    write!(stream, "{line} HTTP/1.1\r\n").unwrap();
    for (k, v) in headers {
        write!(stream, "{k}: {v}\r\n").unwrap();
    }
    write!(stream, "Content-Length: {}\r\n\r\n{body}", body.len()).unwrap();
    let mut lines = BufReader::new(stream).lines().map(Result::unwrap);
    let status = lines
        .next()
        .unwrap()
        .split_whitespace()
        .nth(1)
        .unwrap()
        .parse()
        .unwrap();
    let mut cookie = None;
    for line in lines.take_while(|l| !l.is_empty()) {
        if let Some((k, v)) = line.split_once(':')
            && k.eq_ignore_ascii_case("set-cookie")
        {
            cookie = Some(v.trim().to_owned());
        }
    }
    (status, cookie)
}
