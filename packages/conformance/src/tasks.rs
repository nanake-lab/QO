use crate::{
    Instance, Target, connection::mutations, cursor, decision, id, native_task, start, wire::Client,
};
use serde_json::{Value, json};

fn forbidden_changes(c: &mut Client, task: &str, session: &str, turn: &str, can_cancel: bool) {
    for (method, p) in mutations(task, session, turn) {
        if method != "session.cancel" || !can_cancel {
            c.reject(method, p, "forbidden");
        }
    }
    c.reject("approval.list", json!({"taskId":task}), "forbidden");
}

pub fn ownership_and_close(target: &dyn Target) {
    let s = Instance::start(target, Default::default());
    let mut ui = s.ui();
    let mut owner = s.native();
    let mut other = s.native();
    let ui_task = id();
    let ui_session = id();
    assert_eq!(
        ui.call("task.create", json!({"taskId":ui_task,"name":"Browser"}))["createdBy"],
        "ui"
    );
    ui.call(
        "session.open",
        json!({"taskId":ui_task,"sessionId":ui_session}),
    );
    let ui_turn = start(&mut ui, &ui_session, "input");
    let pending = ui.notice("input.requested");
    forbidden_changes(&mut owner, &ui_task, &ui_session, &ui_turn, false);
    owner.reject(
        "task.attach",
        json!({"taskId":ui_task,"ownerKey":s.owner_key()}),
        "forbidden",
    );
    owner.reject(
        "task.create",
        json!({"taskId":ui_task,"name":"Browser","ownerKey":s.owner_key()}),
        "forbidden",
    );
    ui.call(
        "turn.interrupt",
        json!({"sessionId":ui_session,"turnId":ui_turn}),
    );
    ui.reject(
        "input.answer",
        json!({
            "sessionId": ui_session, "turnId": ui_turn, "requestId": pending["requestId"],
            "answers": {"format": {"answers": ["文本"]}}
        }),
        "request_withdrawn",
    );

    let task = id();
    let key = s.owner_key();
    let session = id();
    let task_value = owner.call(
        "task.create",
        json!({"taskId":task,"ownerKey":key,"name":"Native"}),
    );
    assert_eq!(task_value["createdBy"], "external");
    owner.call("session.open", json!({"taskId":task,"sessionId":session}));
    owner.reject("task.close", json!({"taskId":task}), "conflict");
    let turn = start(&mut owner, &session, "approval");
    let request = owner.notice("approval.requested");
    let vote = decision(&request, "allow_once");
    forbidden_changes(&mut ui, &task, &session, &turn, true);
    ui.reject(
        "task.create",
        json!({"taskId":task,"name":"Native"}),
        "forbidden",
    );
    ui.reject(
        "task.attach",
        json!({"taskId":task,"ownerKey":key}),
        "forbidden",
    );
    ui.reject("approval.decide", vote.clone(), "forbidden");
    other.reject(
        "task.attach",
        json!({"taskId":task,"ownerKey":s.owner_key()}),
        "forbidden",
    );
    other.call("task.attach", json!({"taskId":task,"ownerKey":key}));
    assert_eq!(other.notice("approval.requested"), request);
    owner.reject("approval.decide", vote.clone(), "forbidden");
    owner.reject(
        "session.cancel",
        json!({"taskId":task,"sessionId":session}),
        "forbidden",
    );
    other.reject("task.close", json!({"taskId":task}), "conflict");

    let after = cursor(&ui.state());
    ui.subscribe(after);
    other.subscribe(after);
    let cancelled = ui.call("session.cancel", json!({"taskId":task,"sessionId":session}));
    assert_eq!(cancelled["cancelledBy"], "ui");
    let until_cancel = |e: &Value| {
        e["kind"] == "session.changed"
            && e["payload"]["sessionId"] == session
            && e["payload"]["state"] == "cancelled"
    };
    let events = ui.events_until(until_cancel);
    assert_eq!(other.events_until(until_cancel), events);
    let cancel = events.last().unwrap();
    assert_eq!(cancel["payload"], cancelled);
    let pending_task = ui.state()["tasks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|v| v["taskId"] == task)
        .unwrap()
        .clone();
    assert_eq!(pending_task["state"], "open");
    other.reject("approval.decide", vote, "request_withdrawn");
    let status = other.call(
        "approval.get",
        json!({"taskId":task,"approvalId":request["approvalId"]}),
    );
    assert_eq!(status["state"], "withdrawn");
    assert!(status.get("decision").is_none());
    ui.reject("task.close", json!({"taskId":task}), "forbidden");
    let closed = other.call("task.close", json!({"taskId":task}));
    assert_eq!(closed["state"], "closed");
    let close_event = other.notice("event");
    assert_eq!(close_event["kind"], "task.changed");
    assert_eq!(close_event["payload"], closed);
    assert_eq!(cursor(&close_event), cursor(cancel) + 1);
    other.reject(
        "session.open",
        json!({"taskId":task,"sessionId":id()}),
        "conflict",
    );
    let snapshot = ui.state();
    assert!(snapshot["tasks"].as_array().unwrap().contains(&closed));
    assert!(
        snapshot["sessions"]
            .as_array()
            .unwrap()
            .contains(&cancelled)
    );
    let mut resumed = s.native();
    resumed.subscribe(after);
    let history = resumed.events_until(|e| cursor(e) == cursor(&close_event));
    assert_eq!(&history[..events.len()], events);
    assert_eq!(history.last().unwrap(), &close_event);
}

pub fn idempotent_writes(target: &dyn Target) {
    let s = Instance::start(target, Default::default());
    let mut owner = s.native();
    let mut observer = s.native();
    let (task, session, turn) = (id(), id(), id());
    let create = json!({"taskId":task,"name":"Retry","ownerKey":s.owner_key()});
    let open = json!({"taskId":task,"sessionId":session});
    let begin = json!({"sessionId":session,"turnId":turn,"input":"scenario:approval"});
    let append = json!({"sessionId":session,"turnId":turn,"appendId":id(),"input":"append once"});
    let created = owner.call("task.create", create.clone());
    let opened = owner.call("session.open", open.clone());
    let started = owner.call("turn.start", begin.clone());
    let original = owner.notice("approval.requested");
    let appended = owner.call("turn.append", append.clone());
    let writes = [
        ("task.create", create, created, "name"),
        ("session.open", open, opened, "model"),
        ("turn.start", begin, started, "input"),
        ("turn.append", append, appended, "input"),
    ];
    let snapshot = observer.state();
    for (name, params, first, field) in &writes {
        assert_eq!(owner.call(name, params.clone()), *first);
        let mut changed = params.clone();
        changed[*field] = json!("changed");
        owner.reject(name, changed.clone(), "conflict");
        observer.reject(name, changed, "forbidden");
        observer.reject(name, params.clone(), "forbidden");
    }
    assert_eq!(observer.state(), snapshot);
    assert_eq!(
        owner.call("approval.list", json!({"taskId":task}))["requests"],
        json!([original])
    );
    observer.subscribe(cursor(&snapshot));
    owner.call("approval.decide", decision(&original, "allow_once"));
    let progress = observer.events_until(|event| {
        event["kind"] == "session.changed" && event["payload"]["state"] == "idle"
    });
    let outputs: Vec<_> = progress
        .iter()
        .filter(|event| event["kind"] == "item.delta")
        .map(|event| &event["payload"]["delta"])
        .collect();
    assert_eq!(outputs, [&json!("approved\n")]);
    owner.call("session.finish", json!({"taskId":task,"sessionId":session}));
    owner.call("task.close", json!({"taskId":task}));
    let closed = owner.state();
    for (name, p, first, _) in writes {
        assert_eq!(owner.call(name, p), first);
    }
    assert_eq!(owner.state(), closed);
}

pub fn effective_model(target: &dyn Target) {
    let s = Instance::start(target, Default::default());
    let mut c = s.native();
    let task = native_task(&s, &mut c);
    let session = id();
    let opened = c.call(
        "session.open",
        json!({"taskId":task,"sessionId":session,"model":"initial"}),
    );
    let version = opened["harnessVersion"].as_str().unwrap();
    assert!(!version.is_empty());
    let snapshot = c.state();
    c.subscribe(cursor(&snapshot));
    start(&mut c, &session, "reroute");
    let events =
        c.events_until(|e| e["kind"] == "session.changed" && e["payload"]["state"] == "idle");
    let mut text_seen = false;
    let mut model = None;
    for event in &events {
        if event["kind"] == "item.delta" {
            text_seen = true;
        }
        if event["kind"] == "session.changed" {
            let payload = &event["payload"];
            assert_eq!(payload["harnessVersion"], version);
            if payload["state"] == "running" && payload["model"] != opened["model"] {
                assert!(text_seen);
                model = Some(payload["model"].as_str().unwrap());
            }
        }
    }
    let changed = model.expect("missing model reroute");
    assert_eq!(changed, "mock-rerouted");
    let snapshot = c.state();
    assert_eq!(snapshot["sessions"][0]["model"], changed);
    assert_eq!(snapshot["sessions"][0]["harnessVersion"], version);
}
