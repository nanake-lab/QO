use chrono::{SecondsFormat, Utc};
use qo_harness::{EventSink, HarnessEvent};
use qo_protocol::{ItemDelta, TurnState, Usage, UsageCounts};
use serde_json::{Value, json};
use std::{collections::BTreeMap, io};
use uuid::Uuid;

pub(crate) fn invalid() -> io::Error {
    io::Error::other("invalid Codex protocol frame")
}
fn field<'a>(v: &'a Value, k: &str) -> io::Result<&'a str> {
    v[k].as_str().ok_or_else(invalid)
}
fn now() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true)
}
struct ItemRef {
    id: String,
    started: String,
}
pub struct Wire {
    session: String,
    pub thread: String,
    pub turn: Option<(String, String)>, // QO and Codex IDs belong to different namespaces.
    items: BTreeMap<(String, String), ItemRef>,
}
impl Wire {
    pub fn new(session: String, thread: String) -> Self {
        Self {
            session,
            thread,
            turn: None,
            items: BTreeMap::new(),
        }
    }
    pub fn begin(&mut self, local: String, remote: String) {
        self.turn = Some((local, remote));
        self.items.clear();
    }
    pub fn notify(&mut self, method: &str, p: &Value, sink: &EventSink) -> io::Result<()> {
        if p.get("threadId").is_some_and(|t| t != &self.thread) {
            return Err(invalid());
        }
        if method == "model/rerouted" {
            return sink.send(HarnessEvent::Rerouted {
                model: field(p, "toModel")?.into(),
            });
        }
        let Some((local, remote)) = &self.turn else {
            return Ok(());
        };
        let local = local.clone();
        let upstream = if method == "turn/completed" {
            p["turn"]["id"].as_str()
        } else {
            p["turnId"].as_str()
        };
        if upstream != Some(remote) {
            return Ok(());
        }
        match method {
            "item/started" | "item/completed" => {
                let item = &p["item"];
                let key = field(item, "id")?;
                let complete = method == "item/completed";
                let state = if !complete {
                    "running"
                } else {
                    match item["status"].as_str() {
                        Some("failed" | "declined") => "failed",
                        Some("interrupted") => "interrupted",
                        _ => "completed",
                    }
                };
                match field(item, "type")? {
                    "agentMessage" => self.item(
                        sink,
                        &local,
                        key,
                        "",
                        "assistant_message",
                        state,
                        json!(field(item, "text")?),
                    ),
                    "commandExecution" => {
                        if complete
                            && item["aggregatedOutput"].is_null()
                            && let Some(reference) =
                                self.items.get(&(key.to_owned(), String::new()))
                        {
                            return sink.send(HarnessEvent::CommandEnded {
                                turn: local,
                                item: reference.id.clone(),
                                state: serde_json::from_value(json!(state))?,
                                exit_code: item["exitCode"].as_i64(),
                            });
                        }
                        let mut data = json!({"command":field(item,"command")?,"output":item["aggregatedOutput"].as_str().unwrap_or("")});
                        if let Some(exit) = item["exitCode"].as_i64() {
                            data["exitCode"] = json!(exit);
                        }
                        self.item(sink, &local, key, "", "command_execution", state, data)
                    }
                    "fileChange" => self.changes(sink, &local, key, state, &item["changes"]),
                    _ => Ok(()),
                }
            }
            "item/fileChange/patchUpdated" => {
                self.changes(sink, &local, field(p, "itemId")?, "running", &p["changes"])
            }
            "item/agentMessage/delta" | "item/commandExecution/outputDelta" => {
                let item = self
                    .items
                    .get(&(field(p, "itemId")?.into(), String::new()))
                    .ok_or_else(invalid)?;
                sink.send(HarnessEvent::Delta {
                    turn: local,
                    delta: ItemDelta {
                        item_id: item.id.clone(),
                        delta: field(p, "delta")?.into(),
                    },
                })
            }
            "turn/completed" => {
                let state = match field(&p["turn"], "status")? {
                    "completed" => TurnState::Completed,
                    "interrupted" => TurnState::Interrupted,
                    "failed" => TurnState::Failed,
                    _ => return Err(invalid()),
                };
                let error = p["turn"]["error"]["message"].as_str().map(str::to_owned);
                sink.send(HarnessEvent::TurnEnded {
                    turn: local,
                    state,
                    error,
                })
            }
            "thread/tokenUsage/updated" => {
                for name in ["total", "last"] {
                    let source = &p["tokenUsage"][name];
                    if !source.is_object() {
                        return Err(invalid());
                    }
                    let count = |k: &str| -> io::Result<Option<String>> {
                        if source[k].is_null() {
                            Ok(None)
                        } else {
                            source[k]
                                .as_u64()
                                .map(|x| Some(x.to_string()))
                                .ok_or_else(invalid)
                        }
                    };
                    sink.send(HarnessEvent::Usage(Usage {
                        session_id: self.session.clone(),
                        turn_id: Some(local.clone()),
                        model: None,
                        reasoning_effort: None,
                        service_tier: None,
                        source: format!("thread/tokenUsage/updated.{name}"),
                        observed_at: now(),
                        counts: UsageCounts {
                            total_tokens: count("totalTokens")?,
                            input_tokens: count("inputTokens")?,
                            cached_input_tokens: count("cachedInputTokens")?,
                            cache_write_input_tokens: count("cacheWriteInputTokens")?,
                            output_tokens: count("outputTokens")?,
                            reasoning_output_tokens: count("reasoningOutputTokens")?,
                        },
                    }))?;
                }
                Ok(())
            }
            _ => Ok(()),
        }
    }
    fn changes(
        &mut self,
        sink: &EventSink,
        turn: &str,
        key: &str,
        state: &str,
        changes: &Value,
    ) -> io::Result<()> {
        for change in changes.as_array().ok_or_else(invalid)? {
            let path = field(change, "path")?;
            self.item(
                sink,
                turn,
                key,
                path,
                "file_change",
                state,
                json!({"path":path,"diff":field(change,"diff")?}),
            )?;
        }
        Ok(())
    }
    #[allow(clippy::too_many_arguments)]
    fn item(
        &mut self,
        sink: &EventSink,
        turn: &str,
        key: &str,
        path: &str,
        kind: &str,
        state: &str,
        content: Value,
    ) -> io::Result<()> {
        let entry = self
            .items
            .entry((key.into(), path.into()))
            .or_insert_with(|| ItemRef {
                id: Uuid::new_v4().to_string(),
                started: now(),
            });
        let mut v = json!({"itemId":entry.id,"turnId":turn,"startedAt":entry.started,"state":state,"kind":kind,"content":content});
        if state != "running" {
            v["endedAt"] = json!(now());
        }
        sink.send(HarnessEvent::Item(serde_json::from_value(v)?))
    }
}
