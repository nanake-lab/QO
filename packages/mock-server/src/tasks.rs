use crate::{
    scenario::Playback,
    state::{Ownership, State, id, now},
};
use qo_protocol::*;
use qo_server::{Peer, Role, RpcResult};
use serde_json::{Value, json};
use sha2::{Digest as _, Sha256};
use subtle::ConstantTimeEq;

// Service credentials and task keys have 64 hex digits. Never copy them into a projection.
fn public_text(value: &str) -> String {
    let mut out = String::new();
    let mut start = 0;
    for (i, c) in value
        .char_indices()
        .chain(std::iter::once((value.len(), ' ')))
    {
        if !c.is_ascii_hexdigit() {
            if i - start >= 64 {
                out.push_str("[redacted]");
            } else {
                out.push_str(&value[start..i]);
            }
            if i < value.len() {
                out.push(c);
            }
            start = i + c.len_utf8();
        }
    }
    out
}
fn terminal(s: SessionState) -> bool {
    matches!(
        s,
        SessionState::Completed | SessionState::Failed | SessionState::Cancelled
    )
}

impl State {
    pub fn create(&mut self, peer: Peer, mut p: TaskCreateParams) -> RpcResult {
        let key = p
            .owner_key
            .take()
            .map(|k| <[u8; 32]>::from(Sha256::digest(k)));
        if (peer.role == Role::External) != key.is_some() {
            return Err(ErrorReason::Forbidden.into());
        }
        let creation: [u8; 32] = Sha256::digest(serde_json::to_vec(&p).unwrap()).into();
        if let Some(o) = self.owners.get(&p.task_id) {
            if o.key_hash != key {
                return Err(ErrorReason::Forbidden.into());
            }
            if o.creation != creation {
                return Err(ErrorReason::Conflict.into());
            }
            return Ok(json!(self.tasks[&p.task_id]));
        }
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
                creation,
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
    pub fn open(&mut self, peer: Peer, p: SessionOpenParams) -> RpcResult {
        self.require_owner(peer, &p.task_id)?;
        if self.tasks[&p.task_id].state == TaskState::Closed {
            return Err(ErrorReason::Conflict.into());
        }
        let mut s = Session {
            task_id: p.task_id,
            session_id: id(),
            harness: "mock".into(),
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
    pub fn start(&mut self, peer: Peer, p: TurnStartParams) -> RpcResult {
        let mut s = self
            .sessions
            .get(&p.session_id)
            .ok_or(ErrorReason::NotFound)?
            .clone();
        self.require_owner(peer, &s.task_id)?;
        if s.state != SessionState::Idle {
            return Err(ErrorReason::Conflict.into());
        }
        let script = Playback::for_input(&p.input);
        let t = Turn {
            turn_id: id(),
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
    pub fn change_turn(&mut self, peer: Peer, m: Method, p: Value) -> RpcResult {
        let sid = p["sessionId"].as_str().unwrap();
        let mut s = self.sessions.get(sid).ok_or(ErrorReason::NotFound)?.clone();
        self.require_owner(peer, &s.task_id)?;
        let mut t = self
            .turns
            .get(p["turnId"].as_str().unwrap())
            .filter(|t| t.session_id == sid && t.state == TurnState::Running)
            .ok_or(ErrorReason::Conflict)?
            .clone();
        if m == Method::TurnAppend {
            t.input.push('\n');
            t.input.push_str(&public_text(p["input"].as_str().unwrap()));
            return Ok(self.save_turn(t));
        }
        self.stop_playback(&t);
        t.state = TurnState::Interrupted;
        t.ended_at = Some(now());
        let result = self.save_turn(t);
        s.state = SessionState::Idle;
        self.save_session(s);
        Ok(result)
    }
}
