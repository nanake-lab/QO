use crate::{
    Backend, Failure, Notifications, Peer, Role, RpcResult, Settings,
    harness::*,
    storage::{self, Projection, Result, Store, View},
};
use chrono::{SecondsFormat, Utc};
use qo_protocol::*;
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    io,
    path::PathBuf,
    sync::{Arc, Mutex, mpsc},
    thread,
};
use uuid::Uuid;

mod callbacks;
mod data;
mod tasks;
#[cfg(test)]
mod tests;
mod upstream;
use data::*;

pub(crate) fn now() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true)
}
fn decode<T: serde::de::DeserializeOwned>(v: Value) -> Result<T> {
    serde_json::from_value(v).map_err(|_| ErrorReason::InvalidParams.into())
}
fn terminal(s: SessionState) -> bool {
    matches!(
        s,
        SessionState::Cancelled | SessionState::Completed | SessionState::Failed
    )
}
fn hash(p: &Value) -> [u8; 32] {
    hex::decode(canonical_digest(p))
        .expect("SHA-256 hex")
        .try_into()
        .expect("SHA-256 length")
}
fn public_text(s: &str) -> String {
    // Native credentials, cookies and task keys are all hex-encoded 32-byte secrets.
    let mut out = String::new();
    let mut start = 0;
    for (i, c) in s.char_indices().chain(std::iter::once((s.len(), ' '))) {
        if !c.is_ascii_hexdigit() {
            out.push_str(if i - start >= 64 {
                "[redacted]"
            } else {
                &s[start..i]
            });
            if i < s.len() {
                out.push(c);
            }
            start = i + c.len_utf8();
        }
    }
    out
}

struct Client {
    role: Role,
    out: Notifications,
    subscribed: bool,
}
enum Private {
    Approval(ApprovalRequest, std::time::Instant),
    Input(InputRequested),
}
struct Pending {
    upstream: String,
    material: Private,
}
impl Pending {
    fn session(&self) -> &str {
        match &self.material {
            Private::Approval(a, _) => &a.session_id,
            Private::Input(q) => &q.session_id,
        }
    }
    fn notice(&self) -> (&str, &str, Value) {
        match &self.material {
            Private::Approval(a, _) => (&a.task_id, "approval.requested", json!(a)),
            Private::Input(q) => (&q.task_id, "input.requested", json!(q)),
        }
    }
}

#[derive(Default)]
struct Live {
    clients: BTreeMap<u64, Client>,
    holders: BTreeMap<Id, u64>,
    processes: BTreeMap<Id, Box<dyn Harness>>,
    pending: BTreeMap<Id, Pending>,
    stopped: bool,
}
impl Live {
    fn owns(&self, db: &View<'_>, peer: Peer, task: &str) -> Result<()> {
        let o = db.owner(task)?.ok_or(ErrorReason::NotFound)?;
        if match peer.role {
            Role::Browser => o.key_hash.is_none(),
            Role::External => o.key_hash.is_some() && self.holders.get(task) == Some(&peer.id),
            Role::Desktop => false,
        } {
            Ok(())
        } else {
            Err(ErrorReason::Forbidden.into())
        }
    }
    fn forget_session(&mut self, sid: &str) {
        self.pending.retain(|_, p| p.session() != sid);
        self.processes.remove(sid);
    }
    fn stop(&mut self) {
        self.stopped = true;
        for process in self.processes.values_mut() {
            process.terminate();
        }
        self.processes.clear();
        self.pending.clear();
        for c in self.clients.values() {
            c.out.overflow.send_replace(true);
        }
    }
}

pub struct ServiceBackend {
    store: Store,
    factory: Option<Box<dyn HarnessFactory>>,
    workspaces: PathBuf,
    ttl: chrono::TimeDelta,
    live: Mutex<Live>,
    events: mpsc::Sender<(Id, HarnessEvent)>,
}
impl ServiceBackend {
    pub fn new(
        store: Store,
        settings: &Settings,
        workspaces: PathBuf,
        factory: Option<Box<dyn HarnessFactory>>,
    ) -> io::Result<Arc<Self>> {
        recover(&store).map_err(io::Error::other)?;
        let (events, rx) = mpsc::channel();
        let backend = Arc::new(Self {
            store,
            factory,
            workspaces,
            ttl: settings.approval_ttl()?,
            live: Mutex::default(),
            events,
        });
        let weak = Arc::downgrade(&backend);
        thread::Builder::new()
            .name("qo-harness-events".into())
            .spawn(move || {
                // Wake idle services once per second to persist timeouts even with no owner connected.
                // RPCs check the deadline inside the decision transaction at full precision.
                let interval = std::time::Duration::from_secs(1);
                let mut next_expiry_check = std::time::Instant::now() + interval;
                loop {
                    let message = rx.recv_timeout(
                        next_expiry_check.saturating_duration_since(std::time::Instant::now()),
                    );
                    if matches!(message, Err(mpsc::RecvTimeoutError::Disconnected)) {
                        break;
                    }
                    let Some(backend) = weak.upgrade() else { break };
                    let mut live = backend.live.lock().unwrap();
                    if live.stopped {
                        break;
                    }
                    if std::time::Instant::now() >= next_expiry_check {
                        if backend.expire(&mut live).is_err() {
                            live.stop();
                            break;
                        }
                        next_expiry_check = std::time::Instant::now() + interval;
                    }
                    let Ok((sid, event)) = message else { continue };
                    if !live.processes.contains_key(&sid) {
                        continue;
                    }
                    if let Err(e) = backend.upstream(&mut live, &sid, event) {
                        match e {
                            storage::Error::InvalidEvent
                            | storage::Error::Rejected(_)
                            | storage::Error::Conflict => {
                                if backend
                                    .fail(&mut live, &sid, "invalid_harness_event")
                                    .is_err()
                                {
                                    live.stop();
                                }
                            }
                            _ => live.stop(),
                        }
                    }
                }
            })?;
        Ok(backend)
    }
    fn commit<T>(
        &self,
        l: &Live,
        change: impl FnOnce(&mut Change<'_, '_>) -> Result<T>,
    ) -> Result<T> {
        let (r, events) = self.store.write(|db| {
            let mut c = Change {
                tx: db,
                events: vec![],
            };
            let r = change(&mut c)?;
            Ok((r, c.events))
        })?;
        for client in l.clients.values().filter(|c| c.subscribed) {
            client.out.batch("event", &events);
        }
        Ok(r)
    }
    fn fail(&self, l: &mut Live, sid: &str, reason: &str) -> Result<()> {
        if let Some(h) = l.processes.get_mut(sid) {
            h.terminate();
        }
        self.commit(l, |c| {
            let s = object(c.tx, Projection::Session, sid)?;
            c.end(s, SessionState::Failed, Some(reason.into()), None)
        })?;
        l.forget_session(sid);
        Ok(())
    }
    fn send_pending(&self, l: &Live, peer: Peer, only_task: Option<&str>) -> Result<()> {
        if peer.role == Role::Desktop {
            return Ok(());
        }
        let Some(client) = l.clients.get(&peer.id) else {
            return Ok(());
        };
        self.store.read(|db| {
            for p in l.pending.values() {
                if let Private::Approval(a, deadline) = &p.material
                    && (a.state == ApprovalState::Expired
                        || *deadline <= std::time::Instant::now()
                        || a.expires_at <= now())
                {
                    continue;
                }
                let (task, name, v) = p.notice();
                if only_task.is_some_and(|t| t != task) {
                    continue;
                }
                match l.owns(db, peer, task) {
                    Ok(()) => client.out.push(name, &v),
                    Err(storage::Error::Rejected(ErrorReason::Forbidden)) => {}
                    Err(e) => return Err(e),
                }
            }
            Ok(())
        })
    }
    fn dispatch(&self, l: &mut Live, peer: Peer, m: Method, p: Value) -> Result<Value> {
        if l.stopped {
            return Err(ErrorReason::InternalError.into());
        }
        if peer.role == Role::Desktop {
            return Err(ErrorReason::Forbidden.into());
        }
        match m {
            Method::Initialize | Method::UiLoginLink => Err(ErrorReason::MethodNotFound.into()),
            Method::StateGet => self.store.read(|view| {
                let snapshot = view.snapshot()?;
                Ok(json!({
                    "tasks": snapshot.tasks, "sessions": snapshot.sessions, "turns": snapshot.turns,
                    "items": snapshot.items, "usage": snapshot.usage, "cursor": snapshot.cursor.to_string()
                }))
            }),
            Method::UsageGet => self.store.read(|view| {
                let sid = p["sessionId"].as_str().unwrap();
                let _: Session = object(view, Projection::Session, sid)?;
                let usage = select(view, "SELECT body FROM usage WHERE session=?", sid)?;
                Ok(json!({"usage": usage}))
            }),
            Method::EventsSubscribe => {
                let client = l.clients.get_mut(&peer.id).ok_or(ErrorReason::Forbidden)?;
                if client.subscribed { return Err(ErrorReason::Conflict.into()); }
                let after = p["afterCursor"].as_str().unwrap().parse().map_err(|_| ErrorReason::InvalidParams)?;
                // No commit can publish between pinning H and registering the live queue.
                let replay = self.store.begin_replay(after)?;
                client.out.enqueue(crate::backend::Outbound::Replay(replay));
                client.subscribed = true;
                Ok(json!({}))
            }
            Method::TaskCreate | Method::SessionOpen | Method::TurnStart | Method::TurnAppend => self.write_once(l, peer, m, p),
            Method::TaskAttach => self.attach(l, peer, decode(p)?),
            Method::TaskClose => self.close_task(l, peer, &p),
            Method::SessionFinish | Method::SessionCancel => self.end_session(l, peer, &p, m == Method::SessionCancel),
            Method::TurnInterrupt => self.interrupt(l, peer, decode(p)?),
            Method::ApprovalGet | Method::ApprovalList | Method::ApprovalRenew | Method::ApprovalDecide => self.approval_rpc(l, peer, m, p),
            Method::InputAnswer => self.answer(l, peer, decode(p)?),
        }
    }
}
impl Backend for ServiceBackend {
    fn connected(&self, p: Peer, out: Notifications) {
        let mut l = self.live.lock().unwrap();
        if l.stopped {
            out.overflow.send_replace(true);
            return;
        }
        l.clients.insert(
            p.id,
            Client {
                role: p.role,
                out,
                subscribed: false,
            },
        );
        if p.role == Role::Browser && self.send_pending(&l, p, None).is_err() {
            l.stop();
        }
    }
    fn disconnected(&self, p: Peer) {
        let mut l = self.live.lock().unwrap();
        l.clients.remove(&p.id);
        l.holders.retain(|_, id| *id != p.id);
    }
    fn call(&self, p: Peer, m: Method, args: Value) -> RpcResult {
        let mut l = self.live.lock().unwrap();
        self.dispatch(&mut l, p, m, args).map_err(|e| match e {
            storage::Error::Rejected(r) => r.into(),
            storage::Error::Conflict => ErrorReason::Conflict.into(),
            storage::Error::InvalidCursor => ErrorReason::InvalidParams.into(),
            storage::Error::CursorExpired { oldest, high } => {
                Failure::CursorExpired { oldest, high }
            }
            _ => {
                l.stop();
                ErrorReason::InternalError.into()
            }
        })
    }
}
