use crate::scenario::{Approval, Playback};
use chrono::{SecondsFormat, Utc};
use qo_protocol::*;
use qo_server::{Backend, Failure, Notifications, Peer, Role, RpcResult};
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    sync::Mutex,
};
use uuid::Uuid;

pub fn id() -> String {
    Uuid::new_v4().to_string()
}
pub fn now() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true)
}
// Service credentials and task keys have 64 hex digits. Never copy them into a projection.
pub fn public_text(value: &str) -> String {
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

pub fn decode<T: DeserializeOwned>(v: Value) -> T {
    serde_json::from_value(v).expect("input passed the protocol schema")
}

pub struct Connection {
    pub role: Role,
    pub out: Notifications,
    pub subscribed: bool,
}
pub struct Ownership {
    pub key_hash: Option<[u8; 32]>,
    pub holder: Option<u64>,
}
pub struct Receipt {
    pub method: Method,
    pub params_digest: Digest,
    pub result: Value,
}
pub struct State {
    pub tasks: BTreeMap<Id, Task>,
    pub owners: BTreeMap<Id, Ownership>,
    pub receipts: BTreeMap<Id, Receipt>,
    issued_ids: BTreeSet<Id>,
    pub sessions: BTreeMap<Id, Session>,
    pub turns: BTreeMap<Id, Turn>,
    pub items: BTreeMap<Id, Value>,
    pub usage: BTreeMap<Id, Usage>,
    pub approvals: BTreeMap<Id, Approval>,
    pub decisions: BTreeMap<Id, (Value, Value)>,
    pub inputs: BTreeMap<Id, InputRequested>,
    pub withdrawn_inputs: BTreeMap<Id, (Id, Id)>,
    pub play: BTreeMap<Id, Playback>,
    pub connections: BTreeMap<u64, Connection>,
    events: VecDeque<Value>,
    high: u64,
    retain: Option<usize>,
    pub ttl: chrono::TimeDelta,
    pub workspaces: std::path::PathBuf,
}

pub struct Mock(pub Mutex<State>);
impl Backend for Mock {
    fn connected(&self, peer: Peer, out: Notifications) {
        let mut state = self.0.lock().unwrap();
        state.connections.insert(
            peer.id,
            Connection {
                out,
                role: peer.role,
                subscribed: false,
            },
        );
        if peer.role == Role::Browser {
            state.resend(peer.id, None);
        }
    }
    fn disconnected(&self, peer: Peer) {
        let mut s = self.0.lock().unwrap();
        s.connections.remove(&peer.id);
        for owner in s.owners.values_mut() {
            if owner.holder == Some(peer.id) {
                owner.holder = None;
            }
        }
    }
    fn call(&self, peer: Peer, method: Method, params: Value) -> RpcResult {
        self.0.lock().unwrap().dispatch(peer, method, params)
    }
}
impl State {
    pub fn new(
        retain: Option<usize>,
        ttl: chrono::TimeDelta,
        workspaces: std::path::PathBuf,
    ) -> Self {
        Self {
            tasks: Default::default(),
            owners: Default::default(),
            receipts: Default::default(),
            issued_ids: Default::default(),
            sessions: Default::default(),
            turns: Default::default(),
            items: Default::default(),
            usage: Default::default(),
            approvals: Default::default(),
            decisions: Default::default(),
            inputs: Default::default(),
            withdrawn_inputs: Default::default(),
            play: Default::default(),
            connections: Default::default(),
            events: VecDeque::new(),
            high: 0,
            retain,
            ttl,
            workspaces,
        }
    }
    pub fn id_used(&self, id: &str) -> bool {
        self.receipts.contains_key(id)
            || self.issued_ids.contains(id)
            || self.decisions.contains_key(id)
    }
    pub fn fresh_id(&mut self) -> Id {
        loop {
            let candidate = id();
            if !self.id_used(&candidate) {
                self.issued_ids.insert(candidate.clone());
                return candidate;
            }
        }
    }
    pub fn dispatch(&mut self, peer: Peer, method: Method, p: Value) -> RpcResult {
        if peer.role == Role::Desktop {
            return Err(ErrorReason::Forbidden.into());
        }
        self.expire();
        match method {
            Method::Initialize | Method::UiLoginLink => Err(ErrorReason::MethodNotFound.into()),
            Method::TaskCreate | Method::SessionOpen | Method::TurnStart | Method::TurnAppend => {
                self.write_once(peer, method, p)
            }
            Method::TaskAttach => self.attach(peer, decode(p)),
            Method::TaskClose => self.close_task(peer, p["taskId"].as_str().unwrap()),
            Method::StateGet => Ok(json!({
                "tasks": self.tasks.values().collect::<Vec<_>>(),
                "sessions": self.sessions.values().collect::<Vec<_>>(),
                "turns": self.turns.values().collect::<Vec<_>>(),
                "items": self.items.values().collect::<Vec<_>>(),
                "usage": self.usage.values().collect::<Vec<_>>(),
                "cursor": self.high.to_string()
            })),
            Method::SessionCancel | Method::SessionFinish => self.end_session(
                peer,
                p["taskId"].as_str().unwrap(),
                p["sessionId"].as_str().unwrap(),
                method == Method::SessionCancel,
            ),
            Method::TurnInterrupt => self.interrupt(peer, p),
            Method::ApprovalGet => self.approval_status(peer, decode(p)),
            Method::ApprovalList => self.list(peer, p["taskId"].as_str().unwrap()),
            Method::ApprovalRenew => self.renew(peer, decode(p)),
            Method::ApprovalDecide => self.decide(peer, decode(p)),
            Method::InputAnswer => self.answer(peer, decode(p)),
            Method::EventsSubscribe => self.subscribe(peer, p["afterCursor"].as_str().unwrap()),
            Method::UsageGet => {
                let sid = p["sessionId"].as_str().unwrap();
                if !self.sessions.contains_key(sid) {
                    return Err(ErrorReason::NotFound.into());
                }
                Ok(
                    json!({"usage":self.usage.values().filter(|u| u.session_id == sid).collect::<Vec<_>>()}),
                )
            }
        }
    }

    pub fn event(
        &mut self,
        kind: &str,
        task: &str,
        session: Option<&str>,
        turn: Option<&str>,
        payload: Value,
    ) {
        self.high = self.high.checked_add(1).expect("event cursor overflow");
        let mut event = json!({"kind":kind,"cursor":self.high.to_string(),"at":now(),"taskId":task,"payload":payload});
        if let Some(v) = session {
            event["sessionId"] = json!(v);
        }
        if let Some(v) = turn {
            event["turnId"] = json!(v);
        }
        assert!(validate_output("event", &event), "invalid {kind} event");
        for c in self.connections.values().filter(|c| c.subscribed) {
            c.out.push("event", &event);
        }
        self.events.push_back(event);
        if self.retain.is_some_and(|limit| self.events.len() > limit) {
            self.events.pop_front();
        }
    }
    fn subscribe(&mut self, peer: Peer, cursor: &str) -> RpcResult {
        if self.connections[&peer.id].subscribed {
            return Err(ErrorReason::Conflict.into());
        }
        let cursor = cursor
            .parse::<u64>()
            .map_err(|_| ErrorReason::InvalidParams)?;
        if cursor > self.high {
            return Err(ErrorReason::InvalidParams.into());
        }
        let lower = self.high - self.events.len() as u64;
        if cursor < lower {
            return Err(Failure::CursorExpired {
                oldest: lower + 1,
                high: self.high,
            });
        }
        let connection = self.connections.get_mut(&peer.id).expect("registered peer");
        let replay = self
            .events
            .iter()
            .skip((cursor - lower) as usize)
            .cloned()
            .collect::<Vec<_>>();
        connection.out.batch("event", &replay);
        connection.subscribed = true;
        Ok(json!({}))
    }
    pub fn owns(&self, peer: Peer, task: &str) -> bool {
        self.owners.get(task).is_some_and(|o| match peer.role {
            Role::Browser => o.key_hash.is_none(),
            Role::External => o.key_hash.is_some() && o.holder == Some(peer.id),
            Role::Desktop => false,
        })
    }
    pub fn require_owner(&self, peer: Peer, task: &str) -> Result<(), ErrorReason> {
        if !self.tasks.contains_key(task) {
            return Err(ErrorReason::NotFound);
        }
        if !self.owns(peer, task) {
            return Err(ErrorReason::Forbidden);
        }
        Ok(())
    }
    pub fn private(&self, task: &str, name: &str, value: &Value) {
        assert!(
            validate_output(name, value),
            "invalid private notification: {name}"
        );
        for (&id, c) in &self.connections {
            if self.owns(Peer { id, role: c.role }, task) {
                c.out.push(name, value);
            }
        }
    }
    pub fn resend(&mut self, id: u64, only: Option<&str>) {
        self.expire();
        let c = &self.connections[&id];
        let visible = |task: &str| {
            only.is_none_or(|t| t == task) && self.owns(Peer { id, role: c.role }, task)
        };
        for a in self
            .approvals
            .values()
            .filter(|a| a.request.state == ApprovalState::Pending && visible(&a.request.task_id))
        {
            c.out.push("approval.requested", &json!(a.request));
        }
        for q in self.inputs.values().filter(|q| visible(&q.task_id)) {
            c.out.push("input.requested", &json!(q));
        }
    }
    pub fn save_task(&mut self, t: Task) -> Value {
        let v = json!(t);
        self.event("task.changed", &t.task_id, None, None, v.clone());
        self.tasks.insert(t.task_id.clone(), t);
        v
    }
    pub fn save_session(&mut self, s: Session) -> Value {
        let v = json!(s);
        self.event(
            "session.changed",
            &s.task_id,
            Some(&s.session_id),
            None,
            v.clone(),
        );
        self.sessions.insert(s.session_id.clone(), s);
        v
    }
    pub fn save_turn(&mut self, t: Turn) -> Value {
        let task = self.sessions[&t.session_id].task_id.clone();
        let v = json!(t);
        self.event(
            "turn.changed",
            &task,
            Some(&t.session_id),
            Some(&t.turn_id),
            v.clone(),
        );
        self.turns.insert(t.turn_id.clone(), t);
        v
    }
    pub fn item_event(&mut self, t: &Turn, kind: &str, payload: Value) {
        let task = self.sessions[&t.session_id].task_id.clone();
        self.event(kind, &task, Some(&t.session_id), Some(&t.turn_id), payload);
    }
    pub fn save_item(&mut self, t: &Turn, v: Value) {
        self.item_event(t, "item.changed", v.clone());
        self.items
            .insert(v["itemId"].as_str().unwrap().to_owned(), v);
    }
}
