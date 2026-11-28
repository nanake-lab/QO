//! Deterministic process substitute, compiled only for mock and conformance builds.
use crate::*;
use chrono::{SecondsFormat, Utc};

use qo_protocol::*;
use serde::Deserialize;
use serde_json::json;
use std::{
    io,
    path::PathBuf,
    sync::{
        Arc, Mutex,
        mpsc::{self, Receiver, RecvTimeoutError, Sender},
    },
    thread,
    time::Duration,
};
use uuid::Uuid;

fn now() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true)
}

#[derive(Deserialize)]
#[serde(untagged, deny_unknown_fields)]
enum Step {
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
    Reroute {
        model: String,
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
#[derive(Deserialize)]
struct Counts {
    input: String,
    output: String,
}
fn one() -> usize {
    1
}
fn id() -> String {
    Uuid::new_v4().to_string()
}
fn script(input: &str) -> Vec<Step> {
    macro_rules! scripts {
        ($($name:literal),+) => {match input.split_whitespace().next().and_then(|s|s.strip_prefix("scenario:")){
            $(Some($name)=>include_str!(concat!("../../../mock-server/scenarios/",$name,".json")),)+
            _=>include_str!("../../../mock-server/scenarios/complete.json"),
        }}
    }
    serde_json::from_str(scripts!(
        "approval",
        "approval-twice",
        "file-approval",
        "input",
        "reroute",
        "slow",
        "fail"
    ))
    .expect("valid bundled scripts")
}

#[derive(Default)]
pub struct Scripted {
    /// Crash tests observe the recorded/unsent boundary before killing their isolated child.
    pub delivery_marker: Option<PathBuf>,
}
impl HarnessFactory for Scripted {
    fn name(&self) -> &str {
        "mock"
    }
    fn start(&self, _: &Task, p: &SessionOpenParams, events: EventSink) -> io::Result<Started> {
        Ok(Started {
            version: env!("CARGO_PKG_VERSION").into(),
            model: p.model.clone(),
            reasoning_effort: p.reasoning_effort.clone(),
            service_tier: p.service_tier.clone(),
            process: Box::new(Process {
                model: Arc::new(Mutex::new(p.model.clone())),
                events,
                options: p.clone(),
                replies: None,
                delivery_marker: self.delivery_marker.clone(),
            }),
        })
    }
}
struct Response {
    callback: String,
    reply: Reply,
    accepted: Sender<()>,
}
struct Process {
    model: Arc<Mutex<Option<String>>>,
    events: EventSink,
    options: SessionOpenParams,
    replies: Option<Sender<Response>>,
    delivery_marker: Option<PathBuf>,
}
impl Harness for Process {
    fn start_turn(&mut self, turn: &str, input: &str) -> io::Result<()> {
        let (tx, rx) = mpsc::channel();
        let playback = Playback {
            model: self.model.clone(),
            turn: turn.into(),
            events: self.events.clone(),
            options: self.options.clone(),
            rx,
        };
        let steps = script(input);
        thread::Builder::new()
            .name("qo-script".into())
            .spawn(move || {
                let _ = playback.play(steps);
            })?;
        self.replies = Some(tx);
        Ok(())
    }
    fn append(&mut self, _: &str, _: &str) -> io::Result<()> {
        Ok(())
    } // Script output is fixed.
    fn interrupt(&mut self, _: &str) -> io::Result<()> {
        self.terminate();
        Ok(())
    }
    fn valid_decision(
        &self,
        _: &str,
        kind: ApprovalKind,
        _: Choice,
        answer: Option<&Map<String, Value>>,
    ) -> bool {
        matches!(
            kind,
            ApprovalKind::CommandExecution | ApprovalKind::FileChange
        ) && answer.is_none()
    }
    fn reply(&mut self, callback: &str, reply: Reply) -> io::Result<()> {
        if matches!(reply, Reply::Decision { .. })
            && let Some(path) = &self.delivery_marker
        {
            std::fs::write(path, b"recorded")?;
            // Only the fault-injection launcher supplies this marker. Removing it releases delivery.
            while path.exists() {
                thread::sleep(Duration::from_millis(10));
            }
        }
        let (tx, rx) = mpsc::channel();
        self.replies
            .as_ref()
            .ok_or_else(stopped)?
            .send(Response {
                callback: callback.into(),
                reply,
                accepted: tx,
            })
            .map_err(|_| stopped())?;
        rx.recv().map_err(|_| stopped())
    }
    fn finish(&mut self) -> io::Result<()> {
        self.terminate();
        Ok(())
    }
    fn terminate(&mut self) {
        self.replies.take();
    }
}
fn stopped() -> io::Error {
    io::Error::other("script stopped")
}
struct Playback {
    model: Arc<Mutex<Option<String>>>,
    turn: Id,
    events: EventSink,
    options: SessionOpenParams,
    rx: Receiver<Response>,
}
impl Playback {
    fn changed(&self, v: &serde_json::Value) -> io::Result<()> {
        self.events
            .send(HarnessEvent::Item(serde_json::from_value(v.clone())?))
    }
    fn end_item(&self, mut item: serde_json::Value, state: ItemState) -> io::Result<()> {
        item["state"] = json!(state);
        item["endedAt"] = json!(now());
        self.changed(&item)
    }
    fn item(&self, kind: &str, content: serde_json::Value) -> serde_json::Value {
        json!({"itemId":id(),"turnId":self.turn,"kind":kind,"state":"running","startedAt":now(),"content":content})
    }
    fn delta(&self, item: &serde_json::Value, text: &str) -> io::Result<()> {
        self.events.send(HarnessEvent::Delta {
            turn: self.turn.clone(),
            delta: ItemDelta {
                item_id: item["itemId"].as_str().unwrap().into(),
                delta: text.into(),
            },
        })
    }
    fn wait(&self, callback: &str) -> io::Result<Reply> {
        let r = self.rx.recv().map_err(|_| stopped())?;
        if r.callback != callback {
            return Err(stopped());
        }
        r.accepted.send(()).map_err(|_| stopped())?;
        Ok(r.reply)
    }
    fn operation(
        &self,
        mut item: serde_json::Value,
        approval: bool,
        output: Option<String>,
    ) -> io::Result<()> {
        self.changed(&item)?;
        if approval {
            let (kind, method, params, summary) = if output.is_some() {
                let cmd = item["content"]["command"].as_str().unwrap();
                (
                    ApprovalKind::CommandExecution,
                    "item/commandExecution/requestApproval",
                    json!({"command":cmd}),
                    cmd.to_owned(),
                )
            } else {
                let content = item["content"].clone();
                let summary = format!(
                    "修改 {}\n{}",
                    content["path"].as_str().unwrap(),
                    content["diff"].as_str().unwrap()
                );
                (
                    ApprovalKind::FileChange,
                    "item/fileChange/requestApproval",
                    content,
                    summary,
                )
            };
            let callback = id();
            self.events.send(HarnessEvent::Approval {
                callback: callback.clone(),
                turn: Some(self.turn.clone()),
                item: Some(item["itemId"].as_str().unwrap().into()),
                material: Material {
                    kind,
                    raw: RawRequest {
                        method: method.into(),
                        params: params.as_object().unwrap().clone(),
                        context: None,
                    },
                    summary,
                },
            })?;
            if !matches!(
                self.wait(&callback)?,
                Reply::Decision {
                    choice: Choice::AllowOnce,
                    ..
                }
            ) {
                return self.end_item(item, ItemState::Interrupted);
            }
        }
        if let Some(output) = output {
            self.delta(&item, &output)?;
            item["content"]["output"] = json!(output);
            item["content"]["exitCode"] = json!(0);
        }
        self.end_item(item, ItemState::Completed)
    }
    fn play(self, steps: Vec<Step>) -> io::Result<()> {
        let mut assistant = None;
        for step in steps {
            if !matches!(step, Step::Text { .. })
                && let Some(v) = assistant.take()
            {
                self.end_item(v, ItemState::Completed)?;
            }
            match step {
                Step::Text {
                    text,
                    repeat,
                    delay_ms,
                } => {
                    for _ in 0..repeat {
                        if !matches!(
                            self.rx.recv_timeout(Duration::from_millis(delay_ms)),
                            Err(RecvTimeoutError::Timeout)
                        ) {
                            return Err(stopped());
                        }
                        if assistant.is_none() {
                            let v = self.item("assistant_message", json!(""));
                            self.changed(&v)?;
                            assistant = Some(v);
                        }
                        let v = assistant.as_mut().unwrap();
                        self.delta(v, &text)?;
                        v["content"] = json!(v["content"].as_str().unwrap().to_owned() + &text);
                    }
                }
                Step::Command {
                    command,
                    output,
                    approval,
                } => self.operation(
                    self.item("command_execution", json!({"command":command,"output":""})),
                    approval,
                    Some(output),
                )?,
                Step::File {
                    file,
                    diff,
                    approval,
                } => self.operation(
                    self.item("file_change", json!({"path":file,"diff":diff})),
                    approval,
                    None,
                )?,
                Step::Input { questions } => {
                    let callback = id();
                    self.events.send(HarnessEvent::Input {
                        callback: callback.clone(),
                        turn: self.turn.clone(),
                        questions,
                    })?;
                    if !matches!(self.wait(&callback)?, Reply::Input(_)) {
                        return Err(stopped());
                    }
                }
                Step::Reroute { model } => {
                    *self.model.lock().unwrap() = Some(model.clone());
                    self.events.send(HarnessEvent::Rerouted { model })?;
                }
                Step::Usage { usage } => {
                    let total =
                        usage.input.parse::<u64>().unwrap() + usage.output.parse::<u64>().unwrap();
                    self.events.send(HarnessEvent::Usage(Usage {
                        session_id: self.options.session_id.clone(),
                        turn_id: Some(self.turn.clone()),
                        model: self.model.lock().unwrap().clone(),
                        reasoning_effort: self.options.reasoning_effort.clone(),
                        service_tier: self.options.service_tier.clone(),
                        source: "mock/turn".into(),
                        counts: UsageCounts {
                            total_tokens: Some(total.to_string()),
                            input_tokens: Some(usage.input),
                            output_tokens: Some(usage.output),
                            cached_input_tokens: None,
                            cache_write_input_tokens: None,
                            reasoning_output_tokens: None,
                        },
                        observed_at: now(),
                    }))?;
                }
                Step::Fail { fail } => {
                    return self.events.send(HarnessEvent::TurnEnded {
                        turn: self.turn,
                        state: TurnState::Failed,
                        error: Some(fail),
                    });
                }
                Step::End { end } => {
                    assert!(end);
                    return self.events.send(HarnessEvent::TurnEnded {
                        turn: self.turn,
                        state: TurnState::Completed,
                        error: None,
                    });
                }
            }
        }
        Err(io::Error::other("missing script ending"))
    }
}
