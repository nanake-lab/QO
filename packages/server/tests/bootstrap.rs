use serde_json::{Value, json};
use std::{
    fs,
    io::{BufRead, BufReader, Write},
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

const SERVER: &str = env!("CARGO_BIN_EXE_qo-server");

fn isolated(root: &Path, binary: &str) -> Command {
    let mut cmd = Command::new(binary);
    cmd.env("LOCALAPPDATA", root)
        .env("XDG_STATE_HOME", root.join("state"))
        .env("XDG_RUNTIME_DIR", root.join("run"))
        .env("XDG_CONFIG_HOME", root.join("config"))
        .env("HOME", root);
    cmd
}

fn start(root: &Path, port: u16) -> Server {
    let mut child = Server(
        isolated(root, SERVER)
            .stdout(Stdio::null())
            .spawn()
            .unwrap(),
    );
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        assert!(
            child.0.try_wait().unwrap().is_none(),
            "service exited during startup"
        );
        if endpoint(root).with_file_name("desktop.json").exists()
            && TcpStream::connect(("127.0.0.1", port)).is_ok()
        {
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

// Read headers only: even if an upgrade is incorrectly accepted, report its actual 101 status.
fn http_response(
    port: u16,
    request_line: &str,
    headers: &[(&str, &str)],
    body: &str,
) -> (u16, Option<String>) {
    let mut tcp = TcpStream::connect(("127.0.0.1", port)).unwrap();
    tcp.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
    write!(tcp, "{request_line} HTTP/1.1\r\n").unwrap();
    for (name, value) in headers {
        write!(tcp, "{name}: {value}\r\n").unwrap();
    }
    write!(tcp, "Content-Length: {}\r\n\r\n{body}", body.len()).unwrap();
    let mut lines = BufReader::new(tcp).lines().map(Result::unwrap);
    let status = lines
        .next()
        .unwrap()
        .split_whitespace()
        .nth(1)
        .unwrap()
        .parse()
        .unwrap();
    let mut cookie = None;
    for line in lines.take_while(|line| !line.is_empty()) {
        if let Some((name, value)) = line.split_once(':')
            && name.eq_ignore_ascii_case("set-cookie")
        {
            cookie = Some(value.trim().to_owned());
        }
    }
    (status, cookie)
}

fn exchange(port: u16, code: &str, origin: &str) -> (u16, Option<String>) {
    let authority = format!("127.0.0.1:{port}");
    let headers = [
        ("Host", authority.as_str()),
        ("Origin", origin),
        ("Content-Type", "application/json"),
    ];
    http_response(
        port,
        "POST /auth/exchange",
        &headers,
        &json!({"code":code}).to_string(),
    )
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

fn configure(root: &Path) -> u16 {
    let port = TcpListener::bind(("127.0.0.1", 0))
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
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
    port
}

#[test]
fn browser_and_native_login_across_restart() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let port = configure(root);
    let origin = format!("http://127.0.0.1:{port}");
    let server = start(root, port);
    let link = isolated(root, env!("CARGO_BIN_EXE_qo"))
        .args(["open", "--print"])
        .output()
        .unwrap();
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
        !isolated(root, SERVER)
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

#[test]
fn host_and_origin_guards_return_http_403() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let port = configure(root);
    let _server = start(root, port);
    let authority = format!("127.0.0.1:{port}");
    let origin = format!("http://{authority}");
    let other = format!("example.invalid:{port}");
    let record: Value = serde_json::from_slice(&fs::read(endpoint(root)).unwrap()).unwrap();
    let bearer = format!("Bearer {}", record["token"].as_str().unwrap());
    let issued = isolated(root, env!("CARGO_BIN_EXE_qo"))
        .args(["open", "--print"])
        .output()
        .unwrap();
    assert!(issued.status.success());
    let link = std::str::from_utf8(&issued.stdout).unwrap().trim();
    let code = link.strip_prefix(&format!("{origin}/#login=")).unwrap();
    let body = json!({"code":code}).to_string();

    let host = ("Host", authority.as_str());
    let same_origin = ("Origin", origin.as_str());
    for route in ["GET /", "GET /rpc", "POST /auth/exchange"] {
        let status = |context: &[(&str, &str)]| {
            let mut headers = context.to_vec();
            let payload = match route {
                "GET /rpc" => {
                    headers.extend([
                        ("Connection", "Upgrade"),
                        ("Upgrade", "websocket"),
                        ("Sec-WebSocket-Version", "13"),
                        ("Sec-WebSocket-Key", "dGhlIHNhbXBsZSBub25jZQ=="),
                        ("Authorization", bearer.as_str()),
                    ]);
                    ""
                }
                "POST /auth/exchange" => {
                    headers.push(("Content-Type", "application/json"));
                    body.as_str()
                }
                _ => "",
            };
            http_response(port, route, &headers, payload).0
        };

        assert_eq!(
            status(&[("Host", &other), same_origin]),
            403,
            "{route}: wrong Host"
        );
        assert_eq!(
            status(&[host, host, same_origin]),
            403,
            "{route}: duplicate Host"
        );
        if route == "GET /" {
            assert_eq!(status(&[host]), 200, "normal navigation without Origin");
        } else {
            assert_eq!(status(&[host]), 403, "{route}: missing Origin");
            assert_eq!(
                status(&[host, ("Origin", "http://example.invalid")]),
                403,
                "{route}: wrong Origin"
            );
            assert_eq!(
                status(&[host, same_origin, same_origin]),
                403,
                "{route}: duplicate Origin"
            );
            let expected = if route == "GET /rpc" { 101 } else { 204 };
            assert_eq!(
                status(&[host, same_origin]),
                expected,
                "{route}: valid control"
            );
        }
    }
}

#[cfg(target_os = "linux")]
#[test]
fn qo_open_hands_a_fresh_authenticated_url_to_the_default_browser() {
    use std::os::unix::fs::PermissionsExt;

    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let port = configure(root);
    let _server = start(root, port);
    let origin = format!("http://127.0.0.1:{port}");

    // Stand in for the system URL handler so CI never needs a graphical browser.
    let handler = root.join("xdg-open");
    let received = root.join("received");
    fs::write(
        &handler,
        "#!/bin/sh\numask 077\nprintf '%s' \"$1\" > \"$QO_TEST_RECEIVED\"\n",
    )
    .unwrap();
    fs::set_permissions(&handler, fs::Permissions::from_mode(0o700)).unwrap();
    for _ in 0..2 {
        let result = isolated(root, env!("CARGO_BIN_EXE_qo"))
            .arg("open")
            .env("PATH", root)
            .env("QO_TEST_RECEIVED", &received)
            .output()
            .unwrap();
        assert!(result.status.success());
        assert!(result.stdout.is_empty() && result.stderr.is_empty());
        let url = fs::read_to_string(&received).unwrap();
        let code = url.strip_prefix(&format!("{origin}/#login=")).unwrap();
        assert_eq!(exchange(port, code, &origin).0, 204);
        assert_eq!(exchange(port, code, &origin).0, 403);
        fs::remove_file(&received).unwrap();
    }
}

#[test]
fn newer_database_stops_real_service_before_discovery() {
    let root = tempfile::tempdir().unwrap();
    let state = root.path().join(if cfg!(windows) {
        "QO/state"
    } else {
        "state/qo"
    });
    fs::create_dir_all(&state).unwrap();
    let db = rusqlite::Connection::open(state.join("qo.sqlite3")).unwrap();
    db.execute_batch("PRAGMA user_version = 2").unwrap();
    drop(db);
    let mut server = Server(
        isolated(root.path(), SERVER)
            .stderr(Stdio::piped())
            .spawn()
            .unwrap(),
    );
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(status) = server.0.try_wait().unwrap() {
            assert!(!status.success());
            break;
        }
        assert!(Instant::now() < deadline, "service accepted a newer schema");
        thread::sleep(Duration::from_millis(25));
    }
    let error = std::io::read_to_string(server.0.stderr.take().unwrap()).unwrap();
    assert!(
        error.contains("Version(2)") || error.contains("database version 2"),
        "{error}"
    );
    assert!(!endpoint(root.path()).exists());
}
