use crate::{Instance, Options, Target, cursor, id, native_task, start};
use serde_json::{Value, json};
use std::collections::BTreeMap;

pub fn replay_and_live(target: &dyn Target) {
    let s = Instance::start(target, Default::default());
    let mut writer = s.native();
    let mut view = s.ui();
    let mut reference = s.native();
    assert_eq!(cursor(&view.state()), 0);
    view.subscribe(0);
    reference.subscribe(0);
    let task = native_task(&s, &mut writer);
    let session = id();
    writer.call("session.open", json!({"taskId":task,"sessionId":session}));
    start(&mut writer, &session, "complete");
    let mut actual = view.events_until(|e| e["kind"] == "item.delta");
    let applied = cursor(actual.last().unwrap());
    drop(view);
    let mut expected = reference.events_until(|e| {
        e["kind"] == "session.changed"
            && e["payload"]["state"] == "idle"
            && e["sessionId"] == session
            && cursor(e) > applied
    });
    let high = cursor(expected.last().unwrap());
    assert!(high > applied);

    let mut resumed = s.native();
    resumed.subscribe(applied);
    resumed.reject("events.subscribe", json!({"afterCursor":"0"}), "conflict");
    let live = native_task(&s, &mut writer);
    let marker = |e: &Value| e["kind"] == "task.changed" && e["taskId"] == live;
    expected.extend(reference.events_until(marker));
    actual.extend(resumed.events_until(marker));
    assert_eq!(actual, expected);
    assert_eq!(cursor(actual.last().unwrap()), high + 1);
    assert_eq!(cursor(&resumed.state()), high + 1);
    // Fence the first live delivery as well: a second copy after the marker would arrive here.
    let next_task = native_task(&s, &mut writer);
    let next = resumed.notice("event");
    assert_eq!(next["taskId"], next_task);
    assert_eq!(cursor(&next), high + 2);
    assert_eq!(reference.notice("event"), next);

    let mut kinds = BTreeMap::new();
    let mut text = String::new();
    let mut output = String::new();
    for (i, e) in actual.iter().enumerate() {
        assert_eq!(cursor(e), i as u64 + 1);
        let p = &e["payload"];
        if e["kind"] == "item.changed" {
            kinds.insert(p["itemId"].as_str().unwrap(), &p["kind"]);
        }
        if e["kind"] == "item.delta" {
            let kind = kinds
                .get(p["itemId"].as_str().unwrap())
                .expect("delta without item");
            match kind.as_str().unwrap() {
                "assistant_message" => text.push_str(p["delta"].as_str().unwrap()),
                "command_execution" => output.push_str(p["delta"].as_str().unwrap()),
                other => panic!("unexpected delta for {other}"),
            }
        }
    }
    assert_eq!(text, "收到。 正在检查示例。");
    assert_eq!(output, "demo\n");
}

pub fn retention_and_snapshot(target: &dyn Target) {
    let s = Instance::start(
        target,
        Options {
            retain_events: Some(3),
            ..Default::default()
        },
    );
    let mut writer = s.native();
    let mut viewer = s.ui();
    for _ in 0..6 {
        native_task(&s, &mut writer);
    }
    let state = viewer.state();
    let high = cursor(&state);
    assert_eq!(high, 6);
    assert_eq!(state["tasks"].as_array().unwrap().len(), 6);
    let lower = high - 2;
    let bounds = viewer.reject(
        "events.subscribe",
        json!({"afterCursor":(lower-2).to_string()}),
        "cursor_expired",
    );
    assert_eq!(bounds["oldestCursor"], lower.to_string());
    assert_eq!(bounds["highWater"], high.to_string());
    viewer.reject(
        "events.subscribe",
        json!({"afterCursor":(high+1).to_string()}),
        "invalid_params",
    );
    let mut edge = s.native();
    edge.subscribe(lower - 1);
    for c in lower..=high {
        assert_eq!(cursor(&edge.notice("event")), c);
    }
    viewer.subscribe(high);
    let live = native_task(&s, &mut writer);
    let next = viewer.notice("event");
    assert_eq!(cursor(&next), high + 1);
    assert_eq!(next["taskId"], live);
}
