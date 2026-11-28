//! Black-box conformance checks shared by the real and mock QO servers.
pub mod approvals;
pub mod connection;
pub mod events;
pub mod recovery;
mod target;
pub mod tasks;
mod wire;
pub use target::{Instance, Mock, Options, Real, ScriptedReal, Target, isolated};

use serde_json::{Value, json};
use wire::Client;

fn id() -> String {
    uuid::Uuid::new_v4().to_string()
}

fn cursor(value: &Value) -> u64 {
    value["cursor"].as_str().unwrap().parse().unwrap()
}

fn native_task(instance: &Instance<'_>, client: &mut Client) -> String {
    let task = id();
    client.call(
        "task.create",
        json!({"taskId":task,"name":"Conformance","ownerKey":instance.owner_key()}),
    );
    task
}
fn start(client: &mut Client, session: &str, scenario: &str) -> String {
    let turn = id();
    client.call(
        "turn.start",
        json!({"sessionId":session,"turnId":turn,"input":format!("scenario:{scenario}")}),
    );
    turn
}
fn decision(request: &Value, choice: &str) -> Value {
    json!({
        "decisionId": id(),
        "taskId": request["taskId"],
        "approvalId": request["approvalId"],
        "digest": request["digest"],
        "choice": choice
    })
}
