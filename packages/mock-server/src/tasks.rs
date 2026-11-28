use crate::{
    scenario::Playback,
    state::{Ownership, Receipt, State, decode, now, public_text},
};
use qo_protocol::*;
use qo_server::{Peer, Role, RpcResult};
use serde_json::{Value, json};
use sha2::{Digest as _, Sha256};
use subtle::ConstantTimeEq;

fn terminal(s: SessionState) -> bool {
    matches!(
        s,
        SessionState::Completed | SessionState::Failed | SessionState::Cancelled
    )
}

impl State {
    // Check the target owner before consulting the global ID namespace. All four writes
    // run under the same lock as their receipt, events and script scheduling.
    pub fn write_once(&mut self, peer: Peer, method: Method, p: Value) -> RpcResult {
        let id_field = match method {
            Method::TaskCreate => {
                let task = p["taskId"].as_str().unwrap();
                let key = p.get("ownerKey").and_then(Value::as_str);
                if (peer.role == Role::External) != key.is_some() {
                    return Err(ErrorReason::Forbidden.into());
                }
                if self.tasks.contains_key(task) {
                    self.require_owner(peer, task)?;
                }
                "taskId"
            }
            Method::SessionOpen => {
                self.require_owner(peer, p["taskId"].as_str().unwrap())?;
                "sessionId"
            }
            Method::TurnStart | Method::TurnAppend => {
                let session = self
                    .sessions
                    .get(p["sessionId"].as_str().unwrap())
                    .ok_or(ErrorReason::NotFound)?;
                self.require_owner(peer, &session.task_id)?;
                if method == Method::TurnStart {
                    "turnId"
                } else {
                    "appendId"
                }
            }
            _ => unreachable!("method does not accept a client-generated ID"),
        };
        let id = p[id_field].as_str().unwrap().to_owned();
        let params_digest = canonical_digest(&p);
        if let Some(receipt) = self.receipts.get(&id) {
            return if receipt.method == method && receipt.params_digest == params_digest {
                Ok(receipt.result.clone())
            } else {
                Err(ErrorReason::Conflict.into())
            };
        }
        if self.id_used(&id) {
            return Err(ErrorReason::Conflict.into());
        }
        let result = match method {
            Method::TaskCreate => self.create(peer, decode(p)),
            Method::SessionOpen => self.open(decode(p)),
            Method::TurnStart => self.start(decode(p)),
            Method::TurnAppend => self.append(decode(p)),
            _ => unreachable!(),
        }?;
        self.receipts.insert(
            id,
            Receipt {
                method,
                params_digest,
                result: result.clone(),
            },
        );
        Ok(result)
    }
    fn create(&mut self, peer: Peer, p: TaskCreateParams) -> RpcResult {
        let key = p.owner_key.map(|k| <[u8; 32]>::from(Sha256::digest(k)));
        let t = Task {
            task_id: p.task_id.clone(),
            name: public_text(&p.name),
            group: p.group.as_deref().map(public_text),
            cwd: p.cwd.as_deref().map(public_text).unwrap_or_else(|| {
                self.workspaces
                    .join(&p.task_id)
                    .to_string_lossy()
                    .into_owned()
            }),
            forbidden_roots: p
                .forbidden_roots
                .unwrap_or_default()
                .iter()
                .map(|s| public_text(s))
                .collect(),
            created_by: if peer.role == Role::Browser {
                Creator::Ui
            } else {
                Creator::External
            },
            state: TaskState::Open,
            created_at: now(),
            closed_at: None,
        };
        self.owners.insert(
            p.task_id,
            Ownership {
                key_hash: key,
                holder: key.map(|_| peer.id),
            },
        );
        Ok(self.save_task(t))
    }
    pub fn attach(&mut self, peer: Peer, p: TaskAttachParams) -> RpcResult {
        if peer.role != Role::External {
            return Err(ErrorReason::Forbidden.into());
        }
        let o = self
            .owners
            .get_mut(&p.task_id)
            .ok_or(ErrorReason::NotFound)?;
        let digest: [u8; 32] = Sha256::digest(p.owner_key).into();
        if !o.key_hash.is_some_and(|h| bool::from(h.ct_eq(&digest))) {
            return Err(ErrorReason::Forbidden.into());
        }
        o.holder = Some(peer.id);
        self.resend(peer.id, Some(&p.task_id));
        Ok(json!(self.tasks[&p.task_id]))
    }
    pub fn close_task(&mut self, peer: Peer, id: &str) -> RpcResult {
        self.require_owner(peer, id)?;
        if self
            .sessions
            .values()
            .any(|s| s.task_id == id && !terminal(s.state))
        {
            return Err(ErrorReason::Conflict.into());
        }
        let mut task = self.tasks[id].clone();
        if task.state == TaskState::Closed {
            return Ok(json!(task));
        }
        task.closed_at = Some(now());
        task.state = TaskState::Closed;
        Ok(self.save_task(task))
    }
    fn open(&mut self, p: SessionOpenParams) -> RpcResult {
        if self.tasks[&p.task_id].state == TaskState::Closed {
            return Err(ErrorReason::Conflict.into());
        }
        let mut s = Session {
            task_id: p.task_id,
            session_id: p.session_id,
            harness: "mock".into(),
            harness_version: env!("CARGO_PKG_VERSION").into(),
            model: p.model.as_deref().map(public_text),
            reasoning_effort: p.reasoning_effort.as_deref().map(public_text),
            service_tier: p.service_tier.as_deref().map(public_text),
            state: SessionState::Starting,
            created_at: now(),
            error: None,
            cancelled_by: None,
            ended_at: None,
        };
        self.save_session(s.clone());
        s.state = SessionState::Idle;
        Ok(self.save_session(s))
    }
    pub fn end_session(&mut self, peer: Peer, tid: &str, sid: &str, cancel: bool) -> RpcResult {
        if !(cancel && peer.role == Role::Browser) {
            self.require_owner(peer, tid)?;
        }
        let s = self
            .sessions
            .get(sid)
            .filter(|s| s.task_id == tid)
            .ok_or(ErrorReason::NotFound)?
            .clone();
        if terminal(s.state) || (!cancel && s.state != SessionState::Idle) {
            return Err(ErrorReason::Conflict.into());
        }
        if let Some(t) = self
            .turns
            .values()
            .find(|t| t.session_id == sid && t.state == TurnState::Running)
            .cloned()
        {
            self.stop_playback(&t);
            let mut t = t;
            t.state = TurnState::Interrupted;
            t.ended_at = Some(now());
            self.save_turn(t);
        }
        let mut s = s;
        if cancel {
            s.state = SessionState::Cancelled;
            s.cancelled_by = Some(if peer.role == Role::Browser {
                CancelledBy::Ui
            } else {
                CancelledBy::Owner
            });
        } else {
            s.state = SessionState::Completed;
        }
        s.ended_at = Some(now());
        Ok(self.save_session(s))
    }
    fn start(&mut self, p: TurnStartParams) -> RpcResult {
        let mut s = self
            .sessions
            .get(&p.session_id)
            .ok_or(ErrorReason::NotFound)?
            .clone();
        if s.state != SessionState::Idle {
            return Err(ErrorReason::Conflict.into());
        }
        let script = Playback::for_input(&p.input);
        let t = Turn {
            turn_id: p.turn_id,
            session_id: s.session_id.clone(),
            input: public_text(&p.input),
            state: TurnState::Running,
            started_at: now(),
            ended_at: None,
        };
        s.state = SessionState::Running;
        self.save_session(s);
        self.play.insert(t.turn_id.clone(), script);
        Ok(self.save_turn(t))
    }
    fn running_turn(&self, session: &str, turn: &str) -> Result<Turn, ErrorReason> {
        self.turns
            .get(turn)
            .filter(|t| t.session_id == session && t.state == TurnState::Running)
            .cloned()
            .ok_or(ErrorReason::Conflict)
    }
    fn append(&mut self, p: TurnAppendParams) -> RpcResult {
        let mut t = self.running_turn(&p.session_id, &p.turn_id)?;
        t.input.push('\n');
        t.input.push_str(&public_text(&p.input));
        Ok(self.save_turn(t))
    }
    pub fn interrupt(&mut self, peer: Peer, p: Value) -> RpcResult {
        let sid = p["sessionId"].as_str().unwrap();
        let mut s = self.sessions.get(sid).ok_or(ErrorReason::NotFound)?.clone();
        self.require_owner(peer, &s.task_id)?;
        let mut t = self.running_turn(sid, p["turnId"].as_str().unwrap())?;
        self.stop_playback(&t);
        t.state = TurnState::Interrupted;
        t.ended_at = Some(now());
        let result = self.save_turn(t);
        s.state = SessionState::Idle;
        self.save_session(s);
        Ok(result)
    }
}
