use serde_json::{Value, json};
use std::{
    fs,
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    path::Path,
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, Instant},
};
use tungstenite::{Message, client::IntoClientRequest};

struct Server(Child);
impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn isolated(root: &Path) -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_qo-server"));
    cmd.env("LOCALAPPDATA", root)
        .env("XDG_RUNTIME_DIR", root.join("run"))
        .env("XDG_CONFIG_HOME", root.join("config"))
        .env("HOME", root);
    cmd
}

fn start(root: &Path, port: u16) -> Server {
    let mut child = Server(isolated(root).stdout(Stdio::null()).spawn().unwrap());
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        assert!(
            child.0.try_wait().unwrap().is_none(),
            "service exited during startup"
        );
        if TcpStream::connect(("127.0.0.1", port)).is_ok() && endpoint(root).exists() {
            return child;
        }
        assert!(Instant::now() < deadline, "service startup timed out");
        thread::sleep(Duration::from_millis(25));
    }
}

fn endpoint(root: &Path) -> std::path::PathBuf {
    if cfg!(windows) {
        root.join("QO/run/endpoint.json")
    } else {
        root.join("run/qo/endpoint.json")
    }
}

fn exchange(port: u16, code: &str, origin: &str) -> (u16, Option<String>) {
    let mut tcp = TcpStream::connect(("127.0.0.1", port)).unwrap();
    tcp.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
    let body = json!({"code":code}).to_string();
    write!(tcp,"POST /auth/exchange HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nOrigin: {origin}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).unwrap();
    let mut response = String::new();
    tcp.read_to_string(&mut response).unwrap();
    let status = response.split_whitespace().nth(1).unwrap().parse().unwrap();
    let cookie = response.lines().find_map(|line| {
        let (header, value) = line.split_once(':')?;
        header
            .eq_ignore_ascii_case("set-cookie")
            .then(|| value.trim().to_string())
    });
    (status, cookie)
}

fn upgrade(
    port: u16,
    origin: &str,
    header: &str,
    credential: &str,
    version: &str,
) -> Option<Value> {
    let mut req = format!("ws://127.0.0.1:{port}/rpc")
        .into_client_request()
        .unwrap();
    req.headers_mut().insert("Origin", origin.parse().unwrap());
    req.headers_mut().insert(
        tungstenite::http::HeaderName::from_bytes(header.as_bytes()).unwrap(),
        credential.parse().unwrap(),
    );
    let tcp = TcpStream::connect(("127.0.0.1", port)).unwrap();
    tcp.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
    let (mut ws, _) = tungstenite::client(req, tcp).ok()?;
    ws.send(Message::Text(
        json!({"id":1,"jsonrpc":"2.0","method":"initialize","params":{"versions":[version]}})
            .to_string()
            .into(),
    ))
    .unwrap();
    serde_json::from_str(ws.read().unwrap().to_text().unwrap()).ok()
}

#[test]
fn browser_and_native_login_across_restart() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let port = TcpListener::bind(("127.0.0.1", 0))
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let origin = format!("http://127.0.0.1:{port}");
    let config = if cfg!(windows) {
        root.join("QO/config")
    } else {
        root.join("config/qo")
    };
    fs::create_dir_all(&config).unwrap();
    fs::write(
        config.join("settings.json"),
        json!({"port":port}).to_string(),
    )
    .unwrap();
    let server = start(root, port);
    let link = isolated(root).arg("login-link").output().unwrap();
    assert!(link.status.success(), "native bootstrap failed");
    let link = std::str::from_utf8(&link.stdout).unwrap().trim();
    let code = link.strip_prefix(&format!("{origin}/#login=")).unwrap();
    assert_eq!(exchange(port, code, "http://wrong.invalid").0, 403);
    let (status, cookie) = exchange(port, code, &origin);
    assert_eq!(status, 204);
    let cookie = cookie.unwrap();
    assert!(
        cookie.contains("HttpOnly")
            && cookie.contains("SameSite=Strict")
            && cookie.contains("Path=/rpc")
    );
    let cookie = cookie.split(';').next().unwrap();
    assert_eq!(exchange(port, code, &origin).0, 403);
    assert!(upgrade(port, "http://wrong.invalid", "cookie", cookie, "qo.v1").is_none());
    assert!(upgrade(port, &origin, "authorization", "Bearer invalid", "qo.v1").is_none());
    assert_eq!(
        upgrade(port, &origin, "cookie", cookie, "qo.v1").unwrap()["result"]["version"],
        "qo.v1"
    );
    let record: Value = serde_json::from_slice(&fs::read(endpoint(root)).unwrap()).unwrap();
    let bearer = format!("Bearer {}", record["token"].as_str().unwrap());
    assert_eq!(
        upgrade(port, &origin, "authorization", &bearer, "qo.v2").unwrap()["error"]["data"]["reason"],
        "unsupported_version"
    );
    assert_eq!(
        upgrade(port, &origin, "authorization", &bearer, "qo.v1").unwrap()["result"]["version"],
        "qo.v1"
    );
    assert!(
        !isolated(root)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .unwrap()
            .success()
    );
    drop(server);
    let _new_server = start(root, port);
    assert!(upgrade(port, &origin, "cookie", cookie, "qo.v1").is_none());
    assert!(upgrade(port, &origin, "authorization", &bearer, "qo.v1").is_none());
}
