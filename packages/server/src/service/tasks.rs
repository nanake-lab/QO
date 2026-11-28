use super::*;
use sha2::{Digest as _, Sha256};
use subtle::ConstantTimeEq;

impl ServiceBackend {
    pub(super) fn write_once(
        &self,
        l: &mut Live,
        peer: Peer,
        m: Method,
        p: Value,
    ) -> Result<Value> {
        let params_hash = hash(&p);
        let mut process = None;
        let (reply, fresh) = self.commit(l, |c| {
            let (task, id) = match m {
                Method::TaskCreate => {
                    let task = p["taskId"].as_str().unwrap();
                    if (peer.role == Role::External) != p.get("ownerKey").is_some() {
                        return Err(ErrorReason::Forbidden.into());
                    }
                    if c.tx.owner(task)?.is_some() {
                        l.owns(c.tx, peer, task)?;
                    }
                    (task.to_owned(), task)
                }
                Method::SessionOpen => {
                    let task = p["taskId"].as_str().unwrap();
                    l.owns(c.tx, peer, task)?;
                    (task.to_owned(), p["sessionId"].as_str().unwrap())
                }
                Method::TurnStart | Method::TurnAppend => {
                    let s: Session =
                        object(c.tx, Projection::Session, p["sessionId"].as_str().unwrap())?;
                    l.owns(c.tx, peer, &s.task_id)?;
                    (
                        s.task_id,
                        p[if m == Method::TurnStart {
                            "turnId"
                        } else {
                            "appendId"
                        }]
                        .as_str()
                        .unwrap(),
                    )
                }
                _ => unreachable!(),
            };
            let key = storage::RequestKey {
                id,
                method: m.name(),
                task: &task,
                approval: None,
                params_hash: &params_hash,
            };
            if let Some(first) = c.tx.first_reply(&key)? {
                return Ok((first, false));
            }
            if issued(c.tx, id)? {
                return Err(ErrorReason::Conflict.into());
            }
            let value = match m {
                Method::TaskCreate => self.create_task(c, peer, decode(p.clone())?, params_hash)?,
                Method::SessionOpen => {
                    let (session, handle) = self.open_session(c, decode(p.clone())?)?;
                    process = handle;
                    session
                }
                Method::TurnStart => Self::start_turn(c, decode(p.clone())?)?,
                Method::TurnAppend => {
                    let args: TurnAppendParams = decode(p.clone())?;
                    let s: Session = object(c.tx, Projection::Session, &args.session_id)?;
                    let mut t = active_turn(c.tx, &s.session_id, &args.turn_id)?;
                    t.input.push('\n');
                    t.input.push_str(&public_text(&args.input));
                    c.turn(&s, &t)?
                }
                _ => unreachable!(),
            };
            c.tx.record_reply(&key, &value)?;
            Ok((value, true))
        })?;
        if fresh {
            match m {
                Method::TaskCreate if peer.role == Role::External => {
                    l.holders
                        .insert(p["taskId"].as_str().unwrap().into(), peer.id);
                }
                Method::SessionOpen => {
                    if let Some(h) = process {
                        l.processes
                            .insert(p["sessionId"].as_str().unwrap().into(), h);
                    }
                }
                Method::TurnStart | Method::TurnAppend => {
                    let sid = p["sessionId"].as_str().unwrap();
                    let turn = p["turnId"].as_str().unwrap();
                    let input = p["input"].as_str().unwrap();
                    let h = l.processes.get_mut(sid).ok_or(ErrorReason::InternalError)?;
                    let delivered = if m == Method::TurnStart {
                        h.start_turn(turn, input)
                    } else {
                        h.append(turn, input)
                    };
                    if delivered.is_err() {
                        self.fail(l, sid, "harness_lost")?;
                    }
                }
                _ => {}
            }
        }
        Ok(reply)
    }
    fn create_task(
        &self,
        c: &mut Change<'_, '_>,
        peer: Peer,
        args: TaskCreateParams,
        params_hash: [u8; 32],
    ) -> Result<Value> {
        let id = &args.task_id;
        c.tx.insert_owner(
            id,
            &storage::Owner {
                key_hash: args.owner_key.map(|k| Sha256::digest(k).into()),
                creation_hash: params_hash,
            },
        )?;
        c.task(&Task {
            task_id: id.clone(),
            name: public_text(&args.name),
            group: args.group.as_deref().map(public_text),
            cwd: args
                .cwd
                .as_deref()
                .map(public_text)
                .unwrap_or_else(|| self.workspaces.join(id).to_string_lossy().into_owned()),
            forbidden_roots: args
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
        })
    }
    fn open_session(
        &self,
        c: &mut Change<'_, '_>,
        args: SessionOpenParams,
    ) -> Result<(Value, Option<Box<dyn Harness>>)> {
        let task: Task = object(c.tx, Projection::Task, &args.task_id)?;
        if task.state != TaskState::Open {
            return Err(ErrorReason::Conflict.into());
        }
        let mut s = Session {
            session_id: args.session_id.clone(),
            task_id: task.task_id.clone(),
            harness: self
                .factory
                .as_ref()
                .map_or("unconfigured", |f| f.name())
                .into(),
            harness_version: "unavailable".into(),
            model: None,
            reasoning_effort: None,
            service_tier: None,
            state: SessionState::Starting,
            created_at: now(),
            error: None,
            cancelled_by: None,
            ended_at: None,
        };
        let mut process = None;
        let sink = EventSink::new(args.session_id.clone(), self.events.clone());
        let started = self.factory.as_ref().map(|f| f.start(&task, &args, sink));
        if let Some(Ok(h)) = started {
            s.harness_version = public_text(&h.version);
            s.model = h.model.as_deref().map(public_text);
            s.reasoning_effort = h.reasoning_effort.as_deref().map(public_text);
            s.service_tier = h.service_tier.as_deref().map(public_text);
            c.session(&s)?;
            s.state = SessionState::Idle;
            process = Some(h.process);
        } else {
            c.session(&s)?;
            s.state = SessionState::Failed;
            s.ended_at = Some(now());
            s.error = Some(
                if self.factory.is_none() {
                    "harness_unavailable"
                } else {
                    "harness_start_failed"
                }
                .into(),
            );
        }
        Ok((c.session(&s)?, process))
    }
    fn start_turn(c: &mut Change<'_, '_>, args: TurnStartParams) -> Result<Value> {
        let mut s: Session = object(c.tx, Projection::Session, &args.session_id)?;
        if s.state != SessionState::Idle {
            return Err(ErrorReason::Conflict.into());
        }
        s.state = SessionState::Running;
        c.session(&s)?;
        c.turn(
            &s,
            &Turn {
                turn_id: args.turn_id,
                session_id: args.session_id,
                input: public_text(&args.input),
                state: TurnState::Running,
                started_at: now(),
                ended_at: None,
            },
        )
    }
    pub(super) fn attach(&self, l: &mut Live, peer: Peer, p: TaskAttachParams) -> Result<Value> {
        if peer.role != Role::External {
            return Err(ErrorReason::Forbidden.into());
        }
        let t: Task = self.store.read(|db| {
            let owner = db.owner(&p.task_id)?.ok_or(ErrorReason::NotFound)?;
            let digest: [u8; 32] = Sha256::digest(p.owner_key).into();
            if !owner.key_hash.is_some_and(|k| bool::from(k.ct_eq(&digest))) {
                return Err(ErrorReason::Forbidden.into());
            }
            object(db, Projection::Task, &p.task_id)
        })?;
        l.holders.insert(p.task_id.clone(), peer.id);
        self.send_pending(l, peer, Some(&p.task_id))?;
        Ok(json!(t))
    }
    pub(super) fn close_task(&self, l: &Live, peer: Peer, p: &Value) -> Result<Value> {
        let id = p["taskId"].as_str().unwrap();
        self.commit(l, |c| {
            l.owns(c.tx, peer, id)?;
            let mut t: Task = object(c.tx, Projection::Task, id)?;
            if t.state == TaskState::Closed {
                return Ok(json!(t));
            }
            let active: bool = c.tx.db.query_row(
                "SELECT EXISTS(SELECT 1 FROM sessions WHERE parent=?
                 AND state NOT IN ('completed','cancelled','failed'))",
                [id],
                |r| r.get(0),
            )?;
            if active {
                return Err(ErrorReason::Conflict.into());
            }
            t.state = TaskState::Closed;
            t.closed_at = Some(now());
            c.task(&t)
        })
    }
    pub(super) fn end_session(
        &self,
        l: &mut Live,
        peer: Peer,
        p: &Value,
        cancel: bool,
    ) -> Result<Value> {
        let task = p["taskId"].as_str().unwrap();
        let sid = p["sessionId"].as_str().unwrap();
        let s = self.store.read(|db| {
            if !(cancel && peer.role == Role::Browser) {
                l.owns(db, peer, task)?;
            }
            let s: Session = object(db, Projection::Session, sid)?;
            if s.task_id != task {
                return Err(ErrorReason::NotFound.into());
            }
            if terminal(s.state) || (!cancel && s.state != SessionState::Idle) {
                return Err(ErrorReason::Conflict.into());
            }
            Ok(s)
        })?;
        let h = l.processes.get_mut(sid).ok_or(ErrorReason::InternalError)?;
        let (state, by, error) = if cancel {
            h.terminate();
            (
                SessionState::Cancelled,
                Some(if peer.role == Role::Browser {
                    CancelledBy::Ui
                } else {
                    CancelledBy::Owner
                }),
                None,
            )
        } else if h.finish().is_ok() {
            (SessionState::Completed, None, None)
        } else {
            h.terminate();
            (SessionState::Failed, None, Some("harness_lost".into()))
        };
        let result = self.commit(l, |c| c.end(s, state, error, by))?;
        l.forget_session(sid);
        Ok(result)
    }
    pub(super) fn interrupt(
        &self,
        l: &mut Live,
        peer: Peer,
        p: TurnInterruptParams,
    ) -> Result<Value> {
        let (s, t) = self.store.read(|db| {
            let s: Session = object(db, Projection::Session, &p.session_id)?;
            l.owns(db, peer, &s.task_id)?;
            let t = active_turn(db, &p.session_id, &p.turn_id)?;
            Ok((s, t))
        })?;
        if l.processes
            .get_mut(&p.session_id)
            .ok_or(ErrorReason::InternalError)?
            .interrupt(&p.turn_id)
            .is_err()
        {
            self.fail(l, &p.session_id, "harness_lost")?;
            return Err(ErrorReason::InternalError.into());
        }
        let result = self.commit(l, |c| {
            let result = c.end_turn(&s, t, TurnState::Interrupted)?;
            c.waiting_state(s)?;
            Ok(result)
        })?;
        l.pending.retain(|_, pnd| pnd.session() != p.session_id);
        Ok(result)
    }
}
