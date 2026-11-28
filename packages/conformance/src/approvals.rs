use crate::{Instance, Options, Target, cursor, decision, id, native_task, start};
use serde_json::{Value, json};
use std::{thread, time::Duration};

fn lookup(a: &Value) -> Value {
    json!({"taskId":a["taskId"],"approvalId":a["approvalId"]})
}

pub fn private_decisions_and_single_use(target: &dyn Target) {
    let s = Instance::start(target, Default::default());
    let mut owner = s.native();
    let mut observer = s.ui();
    let task = native_task(&s, &mut owner);
    let session = id();
    owner.call("session.open", json!({"taskId":task,"sessionId":session}));
    let snapshot = observer.state();
    observer.subscribe(cursor(&snapshot));
    start(&mut owner, &session, "approval-twice");
    let first = owner.notice("approval.requested");
    let get = lookup(&first);
    assert_eq!(first["state"], "pending");
    let mut allow = decision(&first, "allow_once");
    observer.reject("approval.get", get.clone(), "forbidden");
    observer.reject("approval.decide", allow.clone(), "forbidden");
    let pending = owner.call("approval.get", get.clone());
    assert_eq!(pending["state"], "pending");
    assert_eq!(pending["digest"], first["digest"]);
    assert!(pending.get("decision").is_none());
    let mut mismatch = allow.clone();
    mismatch["digest"] = json!(if first["digest"] == "0".repeat(64) {
        "1".repeat(64)
    } else {
        "0".repeat(64)
    });
    owner.reject("approval.decide", mismatch, "digest_mismatch");
    assert_eq!(owner.call("approval.get", get.clone()), pending);
    let sent = owner.call("approval.decide", allow.clone());
    assert_eq!(sent["delivery"], "sent");
    assert_eq!(owner.call("approval.decide", allow.clone()), sent);
    allow["choice"] = json!("deny");
    owner.reject("approval.decide", allow, "conflict");
    owner.reject(
        "approval.decide",
        decision(&first, "allow_once"),
        "conflict",
    );
    let second = owner.notice("approval.requested");
    assert_ne!(second["approvalId"], first["approvalId"]);
    assert_ne!(second["itemId"], first["itemId"]);
    assert_eq!(second["turnId"], first["turnId"]);
    assert_eq!(second["raw"]["params"], first["raw"]["params"]);
    let state = observer.state();
    assert_eq!(state["sessions"][0]["state"], "waiting_approval");
    let second_item = state["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["itemId"] == second["itemId"])
        .unwrap();
    assert_eq!(second_item["state"], "running");
    assert_eq!(second_item["content"]["output"], "");
    let denied = owner.call("approval.decide", decision(&second, "deny"));
    assert_eq!(denied["delivery"], "sent");
    let public = observer
        .events_until(|e| e["kind"] == "session.changed" && e["payload"]["state"] == "idle");
    // The terminal event fences the entire interval on this socket. Only public projections
    // can appear there, including the visible command, never the private callback or decision.
    let snapshot = observer.state();
    for data in [json!(public), snapshot.clone()] {
        let text = data.to_string();
        for private in [&first, &second] {
            assert!(!text.contains(private["approvalId"].as_str().unwrap()));
            assert!(!text.contains(private["digest"].as_str().unwrap()));
            assert!(!text.contains(private["raw"]["method"].as_str().unwrap()));
        }
    }
    let items = snapshot["items"].as_array().unwrap();
    let executed = items
        .iter()
        .find(|i| i["itemId"] == first["itemId"])
        .unwrap();
    assert_eq!(executed["state"], "completed");
    assert_eq!(executed["content"]["output"], "approved\n");
    assert_eq!(
        public
            .iter()
            .filter(|e| e["kind"] == "item.delta"
                && e["payload"]["itemId"] == first["itemId"]
                && e["payload"]["delta"] == "approved\n")
            .count(),
        1
    );
    let refused = items
        .iter()
        .find(|i| i["itemId"] == second["itemId"])
        .unwrap();
    assert_eq!(refused["state"], "interrupted");
    assert_eq!(refused["content"]["output"], "");
    owner.call("session.finish", json!({"taskId":task,"sessionId":session}));
    owner.call("task.close", json!({"taskId":task}));
    for (r, d) in [(first, sent), (second, denied)] {
        let status = owner.call("approval.get", lookup(&r));
        assert_eq!(status["state"], "decided");
        assert_eq!(status["digest"], r["digest"]);
        for key in ["decisionId", "choice", "decidedAt", "delivery"] {
            assert_eq!(status["decision"][key], d[key]);
        }
    }
}

pub fn expiry_and_renewal(target: &dyn Target) {
    let s = Instance::start(
        target,
        Options {
            approval_seconds: Some(2),
            ..Default::default()
        },
    );
    let mut owner = s.native();
    let mut ui = s.ui();
    let task = native_task(&s, &mut owner);
    let session = id();
    owner.call("session.open", json!({"taskId":task,"sessionId":session}));
    start(&mut owner, &session, "approval");
    let a = owner.notice("approval.requested");
    let get = lookup(&a);
    thread::sleep(Duration::from_millis(2100));
    owner.reject(
        "approval.decide",
        decision(&a, "allow_once"),
        "approval_expired",
    );
    let expired = owner.call("approval.get", get.clone());
    assert_eq!(expired["state"], "expired");
    assert!(expired.get("decision").is_none());
    let waiting = ui.state();
    assert_eq!(waiting["sessions"][0]["state"], "waiting_approval");
    assert_eq!(waiting["items"][0]["state"], "running");
    assert_eq!(waiting["items"][0]["content"]["output"], "");
    // Wait through another full TTL to catch a delayed automatic grant.
    thread::sleep(Duration::from_millis(2100));
    assert_eq!(ui.state(), waiting);
    let renewed = owner.call("approval.renew", get.clone());
    assert_eq!(owner.notice("approval.requested"), renewed);
    assert_ne!(renewed["approvalId"], a["approvalId"]);
    assert_ne!(renewed["digest"], a["digest"]);
    assert_eq!(renewed["raw"], a["raw"]);
    assert_eq!(renewed["itemId"], a["itemId"]);
    assert_eq!(
        owner.call("approval.list", json!({"taskId":task}))["requests"],
        json!([renewed])
    );
    ui.call("session.cancel", json!({"taskId":task,"sessionId":session}));
    owner.reject(
        "approval.decide",
        decision(&renewed, "allow_once"),
        "request_withdrawn",
    );
    owner.call("task.close", json!({"taskId":task}));
    let withdrawn = owner.call("approval.get", lookup(&renewed));
    assert_eq!(withdrawn["state"], "withdrawn");
    assert!(withdrawn.get("decision").is_none());
    assert_eq!(owner.call("approval.get", get), expired);
}
