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
    io::{self, Write},
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
    streams: BTreeMap<(Id, Id, Id), StreamText>,
    starting: BTreeMap<Id, (EventSink, Vec<HarnessEvent>)>,
    processes: BTreeMap<Id, Box<dyn Harness>>,
    pending: BTreeMap<Id, Pending>,
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
        self.streams.retain(|(session, _, _), _| session != sid);
        self.pending.retain(|_, p| p.session() != sid);
        self.processes.remove(sid);
    }
    fn stop(&mut self, operation: &str, error: storage::Error) -> ! {
        for process in self.processes.values_mut() {
            process.terminate();
        }
        self.processes.clear();
        // SQLite messages can contain application data. Report the code, never the raw message.
        let cause = match error {
            storage::Error::Database(e) => match e.sqlite_error() {
                Some(code) => format!("SQLite {:?} ({})", code.code, code.extended_code),
                None => "SQLite operation failed".to_owned(),
            },
            storage::Error::Io(e) => format!("I/O {:?}", e.kind()),
            other => other.to_string(),
        };
        let _ = writeln!(
            io::stderr().lock(),
            "QO stopped during {operation}: {cause}"
        );
        std::process::exit(1);
    }
}

pub struct ServiceBackend {
    store: Store,
    factory: Option<Box<dyn HarnessFactory>>,
    workspaces: PathBuf,
    secrets: Arc<Secrets>,
    ready: std::sync::Condvar,
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
        let secrets = Arc::new(Secrets::default());
        if let Some(f) = &factory {
            f.register_secrets(&secrets);
        }
        let backend = Arc::new(Self {
            store,
            factory,
            workspaces,
            ttl: settings.approval_ttl()?,
            live: Mutex::default(),
            secrets,
            ready: Default::default(),
            events,
        });
        let batch_bytes = settings.output_batch_bytes;
        let weak = Arc::downgrade(&backend);
        thread::Builder::new()
            .name("qo-harness-events".into())
            .spawn(move || {
                // Wake idle services once per second to persist timeouts even with no owner connected.
                // RPCs check the deadline inside the decision transaction at full precision.
                let interval = std::time::Duration::from_secs(1);
                let mut next_expiry_check = std::time::Instant::now() + interval;
                let mut next = None;
                loop {
                    let message = next.take().map(Ok).unwrap_or_else(|| {
                        rx.recv_timeout(
                            next_expiry_check.saturating_duration_since(std::time::Instant::now()),
                        )
                    });
                    if matches!(message, Err(mpsc::RecvTimeoutError::Disconnected)) {
                        break;
                    }
                    let Some(backend) = weak.upgrade() else { break };
                    let mut live = backend.live.lock().unwrap();
                    if std::time::Instant::now() >= next_expiry_check {
                        if let Err(error) = backend.expire(&mut live) {
                            live.stop("approval expiry", error);
                        }
                        next_expiry_check = std::time::Instant::now() + interval;
                    }
                    let Ok((sid, mut event)) = message else {
                        continue;
                    };
                    if let HarnessEvent::Delta { turn, delta } = &mut event {
                        while delta.delta.len() < batch_bytes {
                            match rx.try_recv() {
                                Ok((id, HarnessEvent::Delta { turn: t, delta: d }))
                                    if id == sid && t == *turn && d.item_id == delta.item_id =>
                                {
                                    delta.delta.push_str(&d.delta)
                                }
                                Ok(other) => {
                                    next = Some(other);
                                    break;
                                }
                                Err(_) => break,
                            }
                        }
                    }
                    if let HarnessEvent::Started(result) = event {
                        backend.complete_start(&mut live, &sid, result);
                        continue;
                    }
                    if let Some((_, early)) = live.starting.get_mut(&sid) {
                        early.push(event);
                        continue;
                    }
                    if !live.processes.contains_key(&sid) {
                        continue;
                    }
                    backend.handle_event(&mut live, &sid, event);
                }
            })?;
        Ok(backend)
    }
    fn complete_start(&self, live: &mut Live, sid: &str, result: io::Result<Started>) {
        if let Some((_, early)) = live.starting.remove(sid) {
            let changed = self.commit(live, |c| {
                let mut s: Session = object(c.tx, Projection::Session, sid)?;
                match &result {
                    Ok(started) => {
                        s.state = SessionState::Idle;
                        s.harness_version = self.secrets.text(&started.version);
                        s.model = started.model.as_deref().map(|s| self.secrets.text(s));
                        s.reasoning_effort = started
                            .reasoning_effort
                            .as_deref()
                            .map(|s| self.secrets.text(s));
                        s.service_tier = started
                            .service_tier
                            .as_deref()
                            .map(|s| self.secrets.text(s));
                    }
                    Err(e) => {
                        s.state = SessionState::Failed;
                        s.error = Some(self.secrets.text(&e.to_string()));
                        s.ended_at = Some(now());
                    }
                }
                c.session(&s)
            });
            if let Err(e) = changed {
                live.stop("completing session startup", e);
            }
            if let Ok(started) = result {
                live.processes.insert(sid.into(), started.process);
                for event in early {
                    if live.processes.contains_key(sid) {
                        self.handle_event(live, sid, event);
                    }
                }
            }
            self.ready.notify_all();
        }
    }
    fn handle_event(&self, live: &mut Live, sid: &str, event: HarnessEvent) {
        if let Err(e) = self.upstream(live, sid, event) {
            match e {
                storage::Error::InvalidEvent
                | storage::Error::Rejected(_)
                | storage::Error::Conflict => {
                    if let Err(error) = self.fail(live, sid, "invalid_harness_event") {
                        live.stop("recording a failed harness session", error);
                    }
                }
                error => live.stop("handling a harness event", error),
            }
        }
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
        let s = self.store.read(|db| object(db, Projection::Session, sid))?;
        self.flush_streams(l, &s, None, None)?;
        self.commit(l, |c| {
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
            Method::SessionOpen => unreachable!(),
            Method::TaskCreate | Method::TurnStart | Method::TurnAppend => self.write_once(l, peer, m, p),
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
    fn remember_secret(&self, secret: &str) {
        self.secrets.remember(secret);
    }
    fn connected(&self, p: Peer, out: Notifications) {
        let mut l = self.live.lock().unwrap();
        l.clients.insert(
            p.id,
            Client {
                role: p.role,
                out,
                subscribed: false,
            },
        );
        if p.role == Role::Browser
            && let Err(error) = self.send_pending(&l, p, None)
        {
            l.stop("restoring pending callbacks", error);
        }
    }
    fn disconnected(&self, p: Peer) {
        let mut l = self.live.lock().unwrap();
        l.clients.remove(&p.id);
        l.holders.retain(|_, id| *id != p.id);
    }
    fn call(&self, p: Peer, m: Method, args: Value) -> RpcResult {
        if matches!(m, Method::TaskCreate | Method::TaskAttach)
            && let Some(key) = args["ownerKey"].as_str()
        {
            self.secrets.remember(key);
        }
        let result = if m == Method::SessionOpen && p.role == Role::Desktop {
            Err(ErrorReason::Forbidden.into())
        } else if m == Method::SessionOpen {
            self.open_session(p, args)
        } else {
            self.dispatch(&mut self.live.lock().unwrap(), p, m, args)
        };
        result.map_err(|e| match e {
            storage::Error::Rejected(r) => r.into(),
            storage::Error::Conflict => ErrorReason::Conflict.into(),
            storage::Error::InvalidCursor => ErrorReason::InvalidParams.into(),
            storage::Error::CursorExpired { oldest, high } => {
                Failure::CursorExpired { oldest, high }
            }
            error => self.live.lock().unwrap().stop(m.name(), error),
        })
    }
}
