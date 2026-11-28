use crate::{Instance, Options, Target, cursor, decision, id, start};
use serde_json::json;
use std::{
    net::TcpStream,
    path::Path,
    thread,
    time::{Duration, Instant},
};

/// Trip the target's storage fault while a turn is waiting for its owner.
pub fn storage_failure(target: &dyn Target, inject: impl FnOnce(&Path)) -> String {
    let mut service = Instance::start(
        target,
        Options {
            capture_stderr: true,
            ..Default::default()
        },
    );
    let mut owner = service.ui();
    let task = id();
    let session = id();
    owner.call(
        "task.create",
        json!({"taskId":task,"name":"Storage failure"}),
    );
    owner.call("session.open", json!({"taskId":task,"sessionId":session}));
    let turn = start(&mut owner, &session, "approval");
    let pending = owner.notice("approval.requested");
    let before = owner.state();

    inject(service.root());
    owner.send_request(
        "turn.append",
        json!({"sessionId":session,"turnId":turn,"appendId":id(),"input":"uncommitted"}),
    );
    let diagnostic = service.wait_for_failure();
    assert!(TcpStream::connect(("127.0.0.1", service.port)).is_err());
    drop(owner);

    service.restart();
    let mut owner = service.ui();
    let after = owner.state();
    assert_eq!(after["sessions"][0]["state"], "failed");
    assert_eq!(after["sessions"][0]["error"], "harness_lost");
    assert_eq!(after["turns"][0]["state"], "failed");
    assert_eq!(after["turns"][0]["input"], before["turns"][0]["input"]);
    assert_eq!(after["items"][0]["state"], "failed");
    let status = owner.call(
        "approval.get",
        json!({"taskId":task,"approvalId":pending["approvalId"]}),
    );
    assert_eq!(status["state"], "withdrawn");
    owner.subscribe(cursor(&before));
    for (index, table) in ["items", "turns", "sessions"].iter().enumerate() {
        let event = owner.notice("event");
        assert_eq!(cursor(&event), cursor(&before) + index as u64 + 1);
        assert_eq!(event["payload"], after[*table][0]);
    }
    assert_eq!(cursor(&after), cursor(&before) + 3);
    diagnostic
}

/// Kill the OS process with a decision committed and its harness write still blocked.
pub fn restart(target: &dyn Target) {
    let mut service = Instance::start(
        target,
        Options {
            pause_delivery: true,
            ..Default::default()
        },
    );
    let mut owner = service.native();
    let task = id();
    let key = service.owner_key();
    let create = json!({"taskId":task,"name":"Recovery","ownerKey":key});
    let first = owner.call("task.create", create.clone());
    let mut open = || {
        let sid = id();
        owner.call("session.open", json!({"taskId":task,"sessionId":sid}));
        sid
    };
    let idle = open();
    let terminal = open();
    let approval = open();
    let input = open();
    let delivering = open();
    owner.call(
        "session.finish",
        json!({"taskId":task,"sessionId":terminal}),
    );
    start(&mut owner, &approval, "approval");
    let pending = owner.notice("approval.requested");
    let turn = start(&mut owner, &input, "input");
    let question = owner.notice("input.requested");
    start(&mut owner, &delivering, "approval");
    let voted = owner.notice("approval.requested");
    let vote = decision(&voted, "allow_once");
    let before = owner.state();
    owner.send_request("approval.decide", vote.clone());
    let deadline = Instant::now() + Duration::from_secs(8);
    while !service.root().join("before-delivery").is_file() {
        assert!(
            Instant::now() < deadline,
            "decision did not reach recorded boundary"
        );
        thread::sleep(Duration::from_millis(10));
    }
    service.restart();
    let mut owner = service.native();
    owner.call("task.attach", json!({"taskId":task,"ownerKey":key}));
    let after = owner.state();
    assert_eq!(owner.call("task.create", create), first);
    for sid in [&approval, &input, &delivering, &idle] {
        let s = after["sessions"]
            .as_array()
            .unwrap()
            .iter()
            .find(|s| s["sessionId"] == *sid)
            .unwrap();
        assert_eq!(s["state"], "failed");
        assert_eq!(s["error"], "harness_lost");
    }
    assert!(
        after["turns"]
            .as_array()
            .unwrap()
            .iter()
            .all(|t| t["state"] == "failed")
    );
    assert!(
        after["items"]
            .as_array()
            .unwrap()
            .iter()
            .all(|i| i["state"] == "failed" && i["content"]["output"] == "")
    );
    assert_eq!(
        owner.call("approval.list", json!({"taskId":task})),
        json!({"requests":[]})
    );
    let lookup = |id: &serde_json::Value| json!({"taskId":task,"approvalId":id});
    assert_eq!(
        owner.call("approval.get", lookup(&pending["approvalId"]))["state"],
        "withdrawn"
    );
    owner.reject(
        "approval.decide",
        decision(&pending, "allow_once"),
        "request_withdrawn",
    );
    owner.reject("input.answer",json!({"requestId":question["requestId"],"sessionId":input,"turnId":turn,"answers":{"note":{"answers":["x"]}}}),"request_withdrawn");
    let rec = owner.call("approval.get", lookup(&voted["approvalId"]));
    assert_eq!(rec["state"], "decided");
    assert_eq!(rec["decision"]["delivery"], "unknown");
    assert_eq!(rec["decision"]["decisionId"], vote["decisionId"]);
    assert_eq!(owner.call("approval.decide", vote)["delivery"], "unknown");

    let mut applied = before;
    let mut last = cursor(&applied);
    assert!(cursor(&after) > last);
    owner.subscribe(last);
    while last < cursor(&after) {
        let event = owner.notice("event");
        last += 1;
        assert_eq!(cursor(&event), last);
        let (table, id) = match event["kind"].as_str().unwrap() {
            "session.changed" => ("sessions", "sessionId"),
            "turn.changed" => ("turns", "turnId"),
            "item.changed" => ("items", "itemId"),
            other => panic!("unexpected recovery event {other}"),
        };
        let payload = &event["payload"];
        assert_ne!(payload["sessionId"], terminal);
        let row = applied[table]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|v| v[id] == payload[id])
            .expect("recovery invented an object");
        *row = payload.clone();
    }
    applied["cursor"] = json!(last.to_string());
    assert_eq!(applied, after);
    service.restart();
    assert_eq!(service.native().state(), after);
}
