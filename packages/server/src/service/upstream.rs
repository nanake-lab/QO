use super::*;
use rusqlite::{OptionalExtension, params};

impl ServiceBackend {
    pub(super) fn upstream(&self, l: &mut Live, sid: &str, event: HarnessEvent) -> Result<()> {
        let s: Session = self.store.read(|db| object(db, Projection::Session, sid))?;
        let item = match &event {
            HarnessEvent::Item(i) => Some(json!(i)),
            _ => None,
        };
        let turn = match &event {
            HarnessEvent::Item(_) => item.as_ref().unwrap()["turnId"].as_str(),
            HarnessEvent::Delta { turn, .. }
            | HarnessEvent::Input { turn, .. }
            | HarnessEvent::TurnEnded { turn, .. } => Some(turn.as_str()),
            HarnessEvent::Approval { turn, .. } => turn.as_deref(),
            _ => None,
        };
        if let Some(t) = turn
            && !self.store.read(|db| {
                Ok(db.db.query_row(
                "SELECT EXISTS(SELECT 1 FROM turns WHERE id=? AND parent=? AND state='running')",
                params![t, sid],
                |r| r.get(0),
            )?)
            })?
        {
            return Ok(()); // An interrupted turn may already have queued its last frames.
        }
        match event {
            HarnessEvent::Item(_) => {
                let mut v = item.unwrap();
                let turn = v["turnId"].as_str().unwrap().to_owned();
                let id = v["itemId"].as_str().unwrap().to_owned();
                match &mut v["content"] {
                    Value::String(s) => *s = public_text(s),
                    Value::Object(fields) => {
                        for val in fields.values_mut() {
                            if let Value::String(s) = val {
                                *s = public_text(s);
                            }
                        }
                    }
                    _ => return Err(storage::Error::InvalidEvent),
                }
                self.commit(l, |c| {
                    let prior: Option<Value> =
                        c.tx.db
                            .query_row("SELECT body FROM items WHERE id=?", [&id], |r| r.get(0))
                            .optional()?;
                    if let Some(old) = prior {
                        if old["turnId"] != turn
                            || old["kind"] != v["kind"]
                            || old["state"] != "running"
                            || old["startedAt"] != v["startedAt"]
                        {
                            return Err(storage::Error::InvalidEvent);
                        }
                    } else if issued(c.tx, &id)? {
                        return Err(storage::Error::InvalidEvent);
                    }
                    c.item(&s, &turn, v)
                })
            }
            HarnessEvent::Delta { turn, delta } => self.commit(l, |c| {
                let mut v: Value = object(c.tx, Projection::Item, &delta.item_id)?;
                if v["turnId"] != turn || v["state"] != "running" {
                    return Err(storage::Error::InvalidEvent);
                }
                let delta_text = public_text(&delta.delta);
                let field = match v["kind"].as_str() {
                    Some("assistant_message") => &mut v["content"],
                    Some("command_execution") => &mut v["content"]["output"],
                    _ => return Err(storage::Error::InvalidEvent),
                };
                let Value::String(content) = field else {
                    return Err(storage::Error::InvalidEvent);
                };
                content.push_str(&delta_text);
                c.tx.put_projection(Projection::Item, &v)?;
                c.event(
                    "item.delta",
                    &s.task_id,
                    Some(sid),
                    Some(&turn),
                    json!({"itemId":delta.item_id,"delta":delta_text}),
                )
            }),
            HarnessEvent::Usage(mut u) => {
                if u.session_id != sid {
                    return Err(storage::Error::InvalidEvent);
                }
                if let Some(tid) = &u.turn_id {
                    let t: Turn = self.store.read(|db| object(db, Projection::Turn, tid))?;
                    if t.session_id != sid {
                        return Err(storage::Error::InvalidEvent);
                    }
                }
                u.model = u.model.as_deref().map(public_text);
                u.service_tier = u.service_tier.as_deref().map(public_text);
                u.reasoning_effort = u.reasoning_effort.as_deref().map(public_text);
                self.commit(l, |c| {
                    let v = json!(u);
                    c.tx.put_projection(Projection::Usage, &v)?;
                    c.event(
                        "usage.updated",
                        &s.task_id,
                        Some(sid),
                        u.turn_id.as_deref(),
                        v,
                    )
                })
            }
            HarnessEvent::Rerouted { model } => self.commit(l, |c| {
                let mut s = s;
                s.model = Some(public_text(&model));
                c.session(&s)?;
                Ok(())
            }),
            HarnessEvent::Approval {
                callback,
                turn,
                item,
                material,
            } => {
                Self::check_callback(l, sid, &callback)?;
                if let Some(id) = &item {
                    let v: Value = self.store.read(|db| object(db, Projection::Item, id))?;
                    let t: Turn = self
                        .store
                        .read(|db| object(db, Projection::Turn, v["turnId"].as_str().unwrap()))?;
                    if t.session_id != sid
                        || turn.as_ref().is_some_and(|id| id != &t.turn_id)
                        || v["state"] != "running"
                    {
                        return Err(storage::Error::InvalidEvent);
                    }
                }
                let a = ApprovalRequest {
                    approval_id: String::new(),
                    task_id: s.task_id,
                    session_id: s.session_id,
                    turn_id: turn,
                    item_id: item,
                    kind: material.kind,
                    raw: material.raw,
                    summary: material.summary,
                    digest: String::new(),
                    expires_at: String::new(),
                    state: ApprovalState::Pending,
                };
                self.issue(l, callback, a, None)?;
                Ok(())
            }
            HarnessEvent::Input {
                callback,
                turn,
                questions,
            } => {
                Self::check_callback(l, sid, &callback)?;
                let id = Uuid::new_v4().to_string();
                let q = InputRequested {
                    request_id: id.clone(),
                    task_id: s.task_id.clone(),
                    session_id: sid.into(),
                    turn_id: turn,
                    questions,
                };
                if !validate_output("input.requested", &json!(q))
                    || q.questions
                        .iter()
                        .map(|q| &q.id)
                        .collect::<std::collections::BTreeSet<_>>()
                        .len()
                        != q.questions.len()
                {
                    return Err(storage::Error::InvalidEvent);
                }
                self.commit(l, |c| {
                    if issued(c.tx, &id)? {
                        return Err(ErrorReason::Conflict.into());
                    }
                    c.tx.db.execute(
                        "INSERT INTO callbacks VALUES (?,?,?,'input','live')",
                        params![id, sid, q.turn_id],
                    )?;
                    c.waiting_state(s)
                })?;
                let p = Pending {
                    upstream: callback,
                    material: Private::Input(q),
                };
                self.notice(l, &p)?;
                l.pending.insert(id, p);
                Ok(())
            }
            HarnessEvent::Withdrawn { callback } => {
                let Some(id) = l
                    .pending
                    .iter()
                    .find(|(_, p)| p.session() == sid && p.upstream == callback)
                    .map(|(id, _)| id.clone())
                else {
                    return Ok(());
                };
                self.commit(l, |c| {
                    c.withdraw(&id)?;
                    c.waiting_state(s)
                })?;
                l.pending.remove(&id);
                Ok(())
            }
            HarnessEvent::TurnEnded { turn, state, error } => {
                if state == TurnState::Running {
                    return Err(storage::Error::InvalidEvent);
                }
                if state == TurnState::Failed || error.is_some() {
                    return self.fail(
                        l,
                        sid,
                        &error
                            .as_deref()
                            .map(public_text)
                            .unwrap_or_else(|| "harness_failed".into()),
                    );
                }
                self.commit(l, |c| {
                    let t = active_turn(c.tx, sid, &turn)?;
                    c.end_turn(&s, t, state)?;
                    c.waiting_state(s)
                })?;
                l.pending.retain(|_, p| p.session() != sid);
                Ok(())
            }
            HarnessEvent::Exited => self.fail(l, sid, "harness_lost"),
        }
    }
    fn check_callback(l: &Live, sid: &str, callback: &str) -> Result<()> {
        if l.pending
            .values()
            .any(|p| p.session() == sid && p.upstream == callback)
        {
            Err(storage::Error::InvalidEvent)
        } else {
            Ok(())
        }
    }
}
