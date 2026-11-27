use crate::state::{State, id, now};
use qo_protocol::*;
use serde::Deserialize;
use serde_json::json;
use std::{
    collections::{HashSet, VecDeque},
    time::{Duration, Instant},
};

#[derive(Clone, Deserialize)]
#[serde(untagged, deny_unknown_fields)]
pub enum Step {
    Text {
        text: String,
        #[serde(default = "one")]
        repeat: usize,
        #[serde(default, rename = "delayMs")]
        delay_ms: u64,
    },
    Command {
        command: String,
        output: String,
        #[serde(default)]
        approval: bool,
    },
    File {
        file: String,
        diff: String,
        #[serde(default)]
        approval: bool,
    },
    Input {
        questions: Vec<InputQuestion>,
    },
    Usage {
        usage: Counts,
    },
    Fail {
        fail: String,
    },
    End {
        end: bool,
    },
}
#[derive(Clone, Deserialize)]
pub struct Counts {
    input: String,
    output: String,
}
fn one() -> usize {
    1
}

pub struct Playback {
    steps: VecDeque<Step>,
    pub waiting: Option<Id>,
    pub due: Instant,
    assistant: Option<Id>,
}
impl Playback {
    pub fn for_input(input: &str) -> Self {
        let name = input
            .split_whitespace()
            .next()
            .unwrap_or("")
            .strip_prefix("scenario:");
        let source = match name {
            Some("approval") => include_str!("../scenarios/approval.json"),
            Some("file-approval") => include_str!("../scenarios/file-approval.json"),
            Some("input") => include_str!("../scenarios/input.json"),
            Some("slow") => include_str!("../scenarios/slow.json"),
            Some("fail") => include_str!("../scenarios/fail.json"),
            _ => include_str!("../scenarios/complete.json"),
        };
        let steps = serde_json::from_str(source).expect("valid bundled harness script");
        let mut state = Self {
            steps,
            waiting: None,
            due: Instant::now(),
            assistant: None,
        };
        state.schedule();
        state
    }
    fn schedule(&mut self) {
        let delay = match self.steps.front() {
            Some(Step::Text { delay_ms, .. }) => *delay_ms,
            _ => 0,
        };
        self.due = Instant::now() + Duration::from_millis(delay);
    }
}

#[derive(Clone)]
pub enum Operation {
    Command { command: String, output: String },
    File { path: String, diff: String },
}
#[derive(Clone)]
pub struct Approval {
    pub request: ApprovalRequest,
    pub operation: Operation,
    pub deadline: Instant,
}

impl State {
    pub fn tick(&mut self) {
        self.expire();
        let ready: Vec<_> = self
            .play
            .iter()
            .filter(|(_, p)| p.waiting.is_none() && p.due <= Instant::now())
            .map(|(id, _)| id.clone())
            .collect();
        for tid in ready {
            self.advance(&tid);
        }
    }
    pub fn advance(&mut self, tid: &str) {
        let mut p = self.play.remove(tid).expect("active script");
        let t = self.turns[tid].clone();
        loop {
            let step = p.steps.pop_front().expect("script must end explicitly");
            if !matches!(step, Step::Text { .. }) {
                self.finish_text(&t, &mut p, ItemState::Completed);
            }
            match step {
                Step::Text {
                    text,
                    repeat,
                    delay_ms,
                } => {
                    assert!(repeat > 0);
                    let item = p
                        .assistant
                        .get_or_insert_with(|| {
                            let item = id();
                            self.save_item(
                                &t,
                                json!({
                                    "itemId": item, "turnId": tid, "kind": "assistant_message",
                                    "state": "running", "startedAt": now(), "content": ""
                                }),
                            );
                            item
                        })
                        .clone();
                    let content = self.items[&item]["content"].as_str().unwrap().to_owned() + &text;
                    self.items.get_mut(&item).unwrap()["content"] = json!(content);
                    self.item_event(&t, "item.delta", json!({"itemId":item,"delta":text}));
                    if repeat > 1 {
                        p.steps.push_front(Step::Text {
                            text,
                            repeat: repeat - 1,
                            delay_ms,
                        });
                    }
                }
                Step::Command {
                    command,
                    output,
                    approval,
                } => {
                    let op = Operation::Command { command, output };
                    if approval {
                        p.waiting = Some(self.issue(&t, id(), op));
                    } else {
                        self.operation_item(&t, &id(), &op, None);
                    }
                }
                Step::File {
                    file,
                    diff,
                    approval,
                } => {
                    let op = Operation::File { path: file, diff };
                    if approval {
                        p.waiting = Some(self.issue(&t, id(), op));
                    } else {
                        self.operation_item(&t, &id(), &op, None);
                    }
                }
                Step::Input { questions } => {
                    let ids: HashSet<_> = questions.iter().map(|q| &q.id).collect();
                    assert_eq!(
                        ids.len(),
                        questions.len(),
                        "ambiguous question IDs in scenario"
                    );
                    let task = self.sessions[&t.session_id].task_id.clone();
                    let q = InputRequested {
                        request_id: id(),
                        questions,
                        task_id: task.clone(),
                        session_id: t.session_id.clone(),
                        turn_id: tid.into(),
                    };
                    self.session_state(&t, SessionState::WaitingInput);
                    self.private(&task, "input.requested", &json!(q));
                    p.waiting = Some(q.request_id.clone());
                    self.inputs.insert(q.request_id.clone(), q);
                }
                Step::Usage { usage } => {
                    let session = &self.sessions[&t.session_id];
                    let total =
                        usage.input.parse::<u64>().unwrap() + usage.output.parse::<u64>().unwrap();
                    let snapshot = Usage {
                        session_id: t.session_id.clone(),
                        turn_id: Some(tid.into()),
                        model: session.model.clone(),
                        service_tier: session.service_tier.clone(),
                        reasoning_effort: session.reasoning_effort.clone(),
                        observed_at: now(),
                        source: "mock/turn".into(),
                        counts: UsageCounts {
                            total_tokens: Some(total.to_string()),
                            input_tokens: Some(usage.input),
                            output_tokens: Some(usage.output),
                            cached_input_tokens: None,
                            reasoning_output_tokens: None,
                            cache_write_input_tokens: None,
                        },
                    };
                    self.item_event(&t, "usage.updated", json!(snapshot));
                    self.usage.insert(t.turn_id.clone(), snapshot);
                }
                Step::End { end } => {
                    assert!(end);
                    self.finish_turn(t, None);
                    return;
                }
                Step::Fail { fail } => {
                    self.finish_turn(t, Some(fail));
                    return;
                }
            }
            p.schedule();
            if p.waiting.is_some() || p.due > Instant::now() {
                break;
            }
        }
        self.play.insert(tid.into(), p);
    }
    fn finish_text(&mut self, t: &Turn, p: &mut Playback, state: ItemState) {
        if let Some(item) = p.assistant.take() {
            let mut v = self.items[&item].clone();
            v["state"] = json!(state);
            v["endedAt"] = json!(now());
            self.save_item(t, v);
        }
    }
    pub fn operation_item(&mut self, t: &Turn, item: &str, op: &Operation, choice: Option<Choice>) {
        let denied = choice == Some(Choice::Deny);
        // Only the owner receives the frozen command or diff from a private callback.
        let (kind, content) = match op {
            Operation::Command { command, .. } => {
                let command = if choice.is_some() {
                    "[withheld]"
                } else {
                    command
                };
                (
                    "command_execution",
                    json!({"command": command, "output": ""}),
                )
            }
            Operation::File { path, diff } if choice.is_none() => {
                ("file_change", json!({"path": path, "diff": diff}))
            }
            Operation::File { .. } => ("file_change", json!({"path": "[withheld]", "diff": ""})),
        };
        let mut record = json!({
            "kind": kind, "itemId": item, "turnId": t.turn_id, "startedAt": now(),
            "state": "running", "content": content
        });
        if let Operation::Command { output, .. } = op {
            if denied {
                record["content"]["output"] = json!("审批已拒绝");
            } else {
                self.save_item(t, record.clone());
                self.item_event(t, "item.delta", json!({"itemId": item, "delta": output}));
                record["content"]["output"] = json!(output);
                record["content"]["exitCode"] = json!(0);
            }
        }
        record["state"] = json!(if denied {
            ItemState::Interrupted
        } else {
            ItemState::Completed
        });
        record["endedAt"] = json!(now());
        self.save_item(t, record);
    }
    pub fn session_state(&mut self, t: &Turn, state: SessionState) {
        let mut s = self.sessions[&t.session_id].clone();
        s.state = state;
        self.save_session(s);
    }
    fn finish_turn(&mut self, mut t: Turn, error: Option<String>) {
        let mut s = self.sessions[&t.session_id].clone();
        t.ended_at = Some(now());
        if error.is_some() {
            t.state = TurnState::Failed;
            s.state = SessionState::Failed;
            s.ended_at = t.ended_at.clone();
            s.error = error;
        } else {
            t.state = TurnState::Completed;
            s.state = SessionState::Idle;
        }
        self.save_turn(t);
        self.save_session(s);
    }
    pub fn stop_playback(&mut self, t: &Turn) {
        if let Some(mut p) = self.play.remove(&t.turn_id) {
            self.finish_text(t, &mut p, ItemState::Interrupted);
            if let Some(waiting) = p.waiting {
                if let Some(a) = self.approvals.get_mut(&waiting) {
                    a.request.state = ApprovalState::Withdrawn;
                }
                if let Some(q) = self.inputs.remove(&waiting) {
                    self.withdrawn_inputs
                        .insert(waiting, (q.session_id, q.turn_id));
                }
            }
        }
    }
    pub fn resume(&mut self, t: &Turn) {
        let p = self.play.get_mut(&t.turn_id).expect("pending turn");
        p.waiting = None;
        p.schedule();
        self.session_state(t, SessionState::Running);
    }
}
