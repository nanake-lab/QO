use super::*;
use rusqlite::{OptionalExtension, params};
use storage::{Projection, View, Write};

pub(super) struct Change<'a, 'db> {
    pub tx: &'a Write<'db>,
    pub events: Vec<Value>,
}
impl Change<'_, '_> {
    pub fn event(
        &mut self,
        kind: &str,
        task: &str,
        session: Option<&str>,
        turn: Option<&str>,
        payload: Value,
    ) -> Result<()> {
        let mut e = json!({"kind":kind,"at":now(),"taskId":task,"payload":payload});
        if let Some(s) = session {
            e["sessionId"] = json!(s);
        }
        if let Some(t) = turn {
            e["turnId"] = json!(t);
        }
        e["cursor"] = json!(self.tx.append_event(e.clone())?.to_string());
        if !validate_output("event", &e) {
            return Err(storage::Error::InvalidEvent);
        }
        self.events.push(e);
        Ok(())
    }
    pub fn task(&mut self, t: &Task) -> Result<Value> {
        let v = json!(t);
        self.tx.put_projection(Projection::Task, &v)?;
        self.event("task.changed", &t.task_id, None, None, v.clone())?;
        Ok(v)
    }
    pub fn session(&mut self, s: &Session) -> Result<Value> {
        let v = json!(s);
        self.tx.put_projection(Projection::Session, &v)?;
        self.event(
            "session.changed",
            &s.task_id,
            Some(&s.session_id),
            None,
            v.clone(),
        )?;
        Ok(v)
    }
    pub fn turn(&mut self, s: &Session, t: &Turn) -> Result<Value> {
        let v = json!(t);
        self.tx.put_projection(Projection::Turn, &v)?;
        self.event(
            "turn.changed",
            &s.task_id,
            Some(&s.session_id),
            Some(&t.turn_id),
            v.clone(),
        )?;
        Ok(v)
    }
    pub fn item(&mut self, s: &Session, turn: &str, v: Value) -> Result<()> {
        self.tx.put_projection(Projection::Item, &v)?;
        self.event(
            "item.changed",
            &s.task_id,
            Some(&s.session_id),
            Some(turn),
            v,
        )
    }
    pub fn withdraw(&self, id: &str) -> Result<()> {
        self.tx.db.execute(
            "UPDATE callbacks SET state='withdrawn' WHERE id=? AND state='live'",
            [id],
        )?;
        self.tx.db.execute(
            "UPDATE approvals SET state='withdrawn' WHERE id=? AND state IN ('pending','expired')",
            [id],
        )?;
        Ok(())
    }
    pub fn end_turn(&mut self, s: &Session, mut t: Turn, state: TurnState) -> Result<Value> {
        let records = select(
            self.tx,
            "SELECT body FROM items WHERE parent=? AND state='running'",
            &t.turn_id,
        )?;
        for mut v in records {
            v["state"] = json!(match state {
                TurnState::Failed => ItemState::Failed,
                TurnState::Completed => ItemState::Completed,
                _ => ItemState::Interrupted,
            });
            v["endedAt"] = json!(now());
            self.item(s, &t.turn_id, v)?;
        }
        self.clear_callbacks(&s.session_id, Some(&t.turn_id))?;
        t.state = state;
        t.ended_at = Some(now());
        self.turn(s, &t)
    }
    pub fn clear_callbacks(&self, sid: &str, turn: Option<&str>) -> Result<()> {
        let mut stmt = self.tx.db.prepare(
            "SELECT id FROM callbacks WHERE session=? AND state='live'
             AND (?2 IS NULL OR turn=?2 OR turn IS NULL)",
        )?;
        let ids = stmt
            .query_map(params![sid, turn], |r| r.get(0))?
            .collect::<rusqlite::Result<Vec<String>>>()?;
        for id in ids {
            self.withdraw(&id)?;
        }
        Ok(())
    }
    pub fn end(
        &mut self,
        mut s: Session,
        state: SessionState,
        error: Option<String>,
        by: Option<CancelledBy>,
    ) -> Result<Value> {
        for t in select(
            self.tx,
            "SELECT body FROM turns WHERE parent=? AND state='running'",
            &s.session_id,
        )? {
            self.end_turn(
                &s,
                decode(t)?,
                if state == SessionState::Failed {
                    TurnState::Failed
                } else {
                    TurnState::Interrupted
                },
            )?;
        }
        self.clear_callbacks(&s.session_id, None)?;
        s.state = state;
        s.error = error;
        s.ended_at = Some(now());
        s.cancelled_by = by;
        self.session(&s)
    }
    pub fn waiting_state(&mut self, mut s: Session) -> Result<()> {
        let kind: Option<String> = self
            .tx
            .db
            .query_row(
                "SELECT kind FROM callbacks WHERE session=? AND state='live' ORDER BY kind LIMIT 1",
                [&s.session_id],
                |r| r.get(0),
            )
            .optional()?;
        let state = match kind.as_deref() {
            Some("approval") => SessionState::WaitingApproval,
            Some("input") => SessionState::WaitingInput,
            _ if self.tx.db.query_row(
                "SELECT EXISTS(SELECT 1 FROM turns WHERE parent=? AND state='running')",
                [&s.session_id],
                |r| r.get(0),
            )? =>
            {
                SessionState::Running
            }
            _ => SessionState::Idle,
        };
        if s.state != state {
            s.state = state;
            self.session(&s)?;
        }
        Ok(())
    }
}

pub(super) fn select(v: &View<'_>, sql: &str, arg: &str) -> Result<Vec<Value>> {
    Ok(v.db
        .prepare(sql)?
        .query_map([arg], |r| r.get(0))?
        .collect::<rusqlite::Result<_>>()?)
}
pub(super) fn object<T: serde::de::DeserializeOwned>(
    v: &View<'_>,
    p: Projection,
    id: &str,
) -> Result<T> {
    let sql = match p {
        Projection::Session => "SELECT body FROM sessions WHERE id=?",
        Projection::Task => "SELECT body FROM tasks WHERE id=?",
        Projection::Turn => "SELECT body FROM turns WHERE id=?",
        Projection::Item => "SELECT body FROM items WHERE id=?",
        Projection::Usage => unreachable!(),
    };
    let body =
        v.db.query_row(sql, [id], |r| r.get(0))
            .optional()?
            .ok_or(ErrorReason::NotFound)?;
    decode(body)
}
pub(super) fn active_turn(v: &View<'_>, sid: &str, tid: &str) -> Result<Turn> {
    let t: Option<Value> =
        v.db.query_row(
            "SELECT body FROM turns WHERE id=? AND parent=? AND state='running'",
            params![tid, sid],
            |r| r.get(0),
        )
        .optional()?;
    decode(t.ok_or(ErrorReason::Conflict)?)
}
pub(super) fn issued(v: &View<'_>, id: &str) -> Result<bool> {
    let sql = "SELECT EXISTS(
        SELECT id FROM callbacks WHERE id=?1 UNION ALL SELECT id FROM items WHERE id=?1
        UNION ALL SELECT id FROM receipts WHERE id=?1 UNION ALL SELECT id FROM approvals WHERE id=?1
    )";
    Ok(v.db.query_row(sql, [id], |row| row.get(0))?)
}
pub(super) fn recover(store: &Store) -> Result<()> {
    store.write(|db| {
        let mut c = Change {
            tx: db,
            events: vec![],
        };
        db.db.execute(
            "UPDATE approvals SET delivery='unknown' WHERE delivery='recorded'",
            [],
        )?;
        let sessions = db
            .db
            .prepare(
                "SELECT body FROM sessions WHERE state NOT IN ('failed','cancelled','completed')",
            )?
            .query_map([], |r| r.get(0))?
            .collect::<rusqlite::Result<Vec<Value>>>()?;
        for s in sessions {
            c.end(
                decode(s)?,
                SessionState::Failed,
                Some("harness_lost".into()),
                None,
            )?;
        }
        Ok(())
    })
}
