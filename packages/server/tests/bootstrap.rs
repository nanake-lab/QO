use qo_conformance::{Instance, Real, Target, isolated};
use std::{
    fs,
    process::{Child, Stdio},
    thread,
    time::{Duration, Instant},
};

const SERVER: &str = env!("CARGO_BIN_EXE_qo-server");
const TARGET: Real = Real(SERVER);

struct Server(Child);
impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[test]
fn qo_open_prints_an_exchangeable_link() {
    let s = Instance::start(&TARGET, Default::default());
    let out = s
        .isolated_command(env!("CARGO_BIN_EXE_qo"))
        .args(["open", "--print"])
        .output()
        .unwrap();
    assert!(out.status.success());
    let link = std::str::from_utf8(&out.stdout).unwrap().trim();
    let code = link
        .strip_prefix(&format!("{}/#login=", s.origin()))
        .expect("local login link");
    assert_eq!(s.exchange(code).0, 204);
}

#[test]
fn second_process_cannot_share_the_instance_lock() {
    let s = Instance::start(&TARGET, Default::default());
    let out = s.isolated_command(SERVER).output().unwrap();
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("instance lock"));
}

#[cfg(target_os = "linux")]
#[test]
fn qo_open_hands_a_fresh_authenticated_url_to_the_default_browser() {
    use std::os::unix::fs::PermissionsExt;

    let s = Instance::start(&TARGET, Default::default());
    let root = s.root();
    let origin = s.origin();

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
        assert_eq!(s.exchange(code).0, 204);
        assert_eq!(s.exchange(code).0, 403);
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
    db.execute_batch("PRAGMA user_version = 3").unwrap();
    drop(db);
    let error = startup_error(root.path(), isolated(root.path(), SERVER));
    assert!(
        error.contains("Version(3)") || error.contains("database version 3"),
        "{error}"
    );
}

fn startup_error(root: &std::path::Path, mut command: std::process::Command) -> String {
    let mut server = Server(command.stderr(Stdio::piped()).spawn().unwrap());
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(status) = server.0.try_wait().unwrap() {
            assert!(!status.success());
            break;
        }
        assert!(
            Instant::now() < deadline,
            "invalid configuration reached service startup"
        );
        thread::sleep(Duration::from_millis(25));
    }
    let error = std::io::read_to_string(server.0.stderr.take().unwrap()).unwrap();
    assert!(!TARGET.discovery(root).join("endpoint.json").exists());
    error
}

#[test]
fn zero_event_limits_are_rejected_before_startup() {
    for field in ["eventRetentionDays", "eventRetentionBytes"] {
        let root = tempfile::tempdir().unwrap();
        let command = TARGET.command(root.path(), 47261, Default::default());
        let config = root.path().join(if cfg!(windows) {
            "QO/config"
        } else {
            "config/qo"
        });
        fs::write(config.join("settings.json"), format!(r#"{{"{field}":0}}"#)).unwrap();
        assert!(startup_error(root.path(), command).contains(field));
    }
}
