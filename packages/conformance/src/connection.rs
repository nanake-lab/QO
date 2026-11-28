use crate::{Instance, Options, Target, id, wire::http};
use serde_json::{Value, json};
use std::{thread, time::Duration};

fn upgrade(port: u16, context: &[(&str, &str)], auth: Option<(&str, &str)>) -> u16 {
    let mut h = context.to_vec();
    h.extend([
        ("Connection", "Upgrade"),
        ("Upgrade", "websocket"),
        ("Sec-WebSocket-Version", "13"),
        ("Sec-WebSocket-Key", "dGhlIHNhbXBsZSBub25jZQ=="),
    ]);
    h.extend(auth);
    http(port, "GET /rpc", &h, "").0
}

pub fn headers_and_credentials(target: &dyn Target) {
    let s = Instance::start(target, Options::default());
    let authority = format!("127.0.0.1:{}", s.port);
    let origin = s.origin();
    let bearer = s.bearer(false);
    let host = ("Host", authority.as_str());
    let same = ("Origin", origin.as_str());
    let auth = ("Authorization", bearer.as_str());
    let body = json!({"code":s.login_code()}).to_string();
    for route in ["GET /", "GET /rpc", "POST /auth/exchange"] {
        let status = |headers: &[(&str, &str)]| match route {
            "GET /rpc" => upgrade(s.port, headers, Some(auth)),
            "POST /auth/exchange" => {
                let mut h = headers.to_vec();
                h.push(("Content-Type", "application/json"));
                http(s.port, route, &h, &body).0
            }
            _ => http(s.port, route, headers, "").0,
        };
        assert_eq!(status(&[same]), 403, "{route} missing Host");
        assert_eq!(
            status(&[("Host", "example.invalid"), same]),
            403,
            "{route} wrong Host"
        );
        assert_eq!(status(&[host, host, same]), 403, "{route} duplicate Host");
        if route == "GET /" {
            assert_eq!(status(&[host]), 200);
        } else {
            assert_eq!(status(&[host]), 403, "{route} missing Origin");
            assert_eq!(
                status(&[host, ("Origin", "http://example.invalid")]),
                403,
                "{route} wrong Origin"
            );
            assert_eq!(status(&[host, same, same]), 403, "{route} duplicate Origin");
            assert_eq!(
                status(&[host, same]),
                if route == "GET /rpc" { 101 } else { 204 }
            );
        }
    }
    for credential in [
        None,
        Some(("Authorization", "Bearer invalid")),
        Some(("Cookie", "qo_session=invalid")),
    ] {
        assert_eq!(upgrade(s.port, &[host, same], credential), 403);
    }
}

// Valid modifying calls for a task/session/turn. Authorization must precede business state checks.
pub(crate) fn mutations(task: &str, session: &str, turn: &str) -> Vec<(&'static str, Value)> {
    vec![
        ("task.close", json!({"taskId":task})),
        ("session.open", json!({"taskId":task,"sessionId":id()})),
        ("session.finish", json!({"taskId":task,"sessionId":session})),
        ("session.cancel", json!({"taskId":task,"sessionId":session})),
        (
            "turn.start",
            json!({"sessionId":session,"turnId":id(),"input":"hello"}),
        ),
        (
            "turn.append",
            json!({"sessionId":session,"turnId":turn,"appendId":id(),"input":"hello"}),
        ),
        ("turn.interrupt", json!({"sessionId":session,"turnId":turn})),
        (
            "input.answer",
            json!({"sessionId":session,"turnId":turn,"requestId":id(),"answers":{"note":{"answers":["hello"]}}}),
        ),
        ("approval.renew", json!({"taskId":task,"approvalId":id()})),
        (
            "approval.decide",
            json!({"taskId":task,"approvalId":id(),"decisionId":id(),"digest":"0".repeat(64),"choice":"deny"}),
        ),
    ]
}

pub fn negotiation_and_roles(target: &dyn Target) {
    let s = Instance::start(target, Options::default());
    let native = s.bearer(false);
    let mut early = s.raw_client("Authorization", &native);
    early.reject("state.get", json!({}), "initialize_required");
    early.expect_closed();
    let mut incompatible = s.raw_client("Authorization", &native);
    incompatible.reject(
        "initialize",
        json!({"versions":["qo.v2"]}),
        "unsupported_version",
    );
    incompatible.expect_closed();
    let mut desktop = s.desktop();
    let (task, session, turn) = (id(), id(), id());
    let mut calls = mutations(&task, &session, &turn);
    calls.extend([
        (
            "task.create",
            json!({"taskId":task,"name":"desktop","ownerKey":s.owner_key()}),
        ),
        (
            "task.attach",
            json!({"taskId":task,"ownerKey":s.owner_key()}),
        ),
        ("approval.get", json!({"taskId":task,"approvalId":id()})),
    ]);
    for (name, p) in calls {
        desktop.reject(name, p, "forbidden");
    }
    s.native().reject("ui.loginLink", json!({}), "forbidden");
    s.ui().reject("ui.loginLink", json!({}), "forbidden");
}

pub fn restart_and_single_use(target: &dyn Target) {
    let mut s = Instance::start(target, Options::default());
    let code = s.login_code();
    let (status, cookie) = s.exchange(&code);
    assert_eq!(status, 204);
    let cookie = cookie.unwrap();
    assert_eq!(s.exchange(&code).0, 403);
    let host = format!("127.0.0.1:{}", s.port);
    assert_eq!(
        upgrade(
            s.port,
            &[("Host", &host), ("Origin", "http://example.invalid")],
            Some(("Cookie", &cookie))
        ),
        403
    );
    s.raw_client("Cookie", &cookie).initialize();
    let native = s.bearer(false);
    let desktop = s.bearer(true);
    s.restart();
    let host = format!("127.0.0.1:{}", s.port);
    let origin = s.origin();
    for old in [
        ("Cookie", cookie.as_str()),
        ("Authorization", native.as_str()),
        ("Authorization", desktop.as_str()),
    ] {
        assert_eq!(
            upgrade(s.port, &[("Host", &host), ("Origin", &origin)], Some(old)),
            403
        );
    }
    s.native();
    s.ui();
}

pub fn link_expiry(target: &dyn Target) {
    let s = Instance::start(
        target,
        Options {
            login_seconds: Some(1),
            ..Default::default()
        },
    );
    let stale = s.login_code();
    thread::sleep(Duration::from_millis(1100));
    assert_eq!(s.exchange(&stale).0, 403);
    assert_eq!(s.exchange(&s.login_code()).0, 204);
}
