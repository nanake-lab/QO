use super::*;
use rusqlite::{OptionalExtension, params};

fn status(a: &storage::Approval) -> Value {
    let mut v = json!({"approvalId":a.id,"taskId":a.task,"sessionId":a.session,"kind":a.kind,"state":a.state,"expiresAt":a.expires_at,"digest":a.digest});
    if let Some(d) = &a.decision {
        v["decision"] = json!({"decisionId":d.id,"choice":d.choice,"decidedAt":d.decided_at,"delivery":d.delivery});
    }
    v
}
fn lookup(db: &View<'_>, id: &str, task: &str) -> Result<storage::Approval> {
    Ok(db
        .approval(id)?
        .filter(|a| a.task == task)
        .ok_or(ErrorReason::NotFound)?)
}
fn decision_reply(a: &storage::Approval, p: &ApprovalDecideParams) -> Result<Value> {
    let d = a.decision.as_ref().ok_or(ErrorReason::InternalError)?;
    let mut v = json!({"decisionId":d.id,"approvalId":a.id,"digest":a.digest,"choice":d.choice,"decidedAt":d.decided_at,"delivery":d.delivery});
    if let Some(answer) = &p.answer {
        v["answer"] = json!(answer);
    }
    Ok(v)
}

impl ServiceBackend {
    pub(super) fn expire(&self, live: &mut Live) -> Result<()> {
        let clock = std::time::Instant::now();
        let utc = now();
        let due: Vec<_> = live
            .pending
            .values_mut()
            .filter_map(|p| match &mut p.material {
                Private::Approval(request, deadline)
                    if request.state == ApprovalState::Pending
                        && (*deadline <= clock || request.expires_at <= utc) =>
                {
                    Some(request)
                }
                _ => None,
            })
            .collect();
        if due.is_empty() {
            return Ok(());
        }
        self.store.write(|tx| {
            for request in &due {
                tx.db.execute(
                    "UPDATE approvals SET state='expired' WHERE id=? AND state='pending'",
                    [&request.approval_id],
                )?;
            }
            Ok(())
        })?;
        for request in due {
            request.state = ApprovalState::Expired;
        }
        Ok(())
    }
    pub(super) fn notice(&self, l: &Live, p: &Pending) -> Result<()> {
        let (task, name, v) = p.notice();
        self.store.read(|db| {
            for (&id, c) in &l.clients {
                match l.owns(db, Peer { id, role: c.role }, task) {
                    Ok(()) => c.out.push(name, &v),
                    Err(storage::Error::Rejected(ErrorReason::Forbidden)) => {}
                    Err(e) => return Err(e),
                }
            }
            Ok(())
        })
    }
    pub(super) fn issue(
        &self,
        l: &mut Live,
        upstream: String,
        mut a: ApprovalRequest,
        replaces: Option<&str>,
    ) -> Result<Value> {
        a.approval_id = Uuid::new_v4().to_string();
        a.expires_at = (Utc::now() + self.ttl).to_rfc3339_opts(SecondsFormat::Millis, true);
        a.state = ApprovalState::Pending;
        a.digest = approval_digest(&a);
        let wire = json!(a);
        if !validate_output("approval.requested", &wire) {
            return Err(storage::Error::InvalidEvent);
        }
        self.commit(l, |c| {
            if let Some(old) = replaces {
                c.tx.db
                    .execute("UPDATE callbacks SET state='resolved' WHERE id=?", [old])?;
            }
            if issued(c.tx, &a.approval_id)? {
                return Err(ErrorReason::Conflict.into());
            }
            c.tx.db.execute(
                "INSERT INTO callbacks VALUES (?,?,?,'approval','live')",
                params![a.approval_id, a.session_id, a.turn_id],
            )?;
            c.tx.put_approval(&storage::Approval {
                id: a.approval_id.clone(),
                task: a.task_id.clone(),
                session: a.session_id.clone(),
                kind: wire["kind"].as_str().unwrap().into(),
                state: "pending".into(),
                expires_at: a.expires_at.clone(),
                digest: a.digest.clone(),
                decision: None,
            })?;
            c.waiting_state(object(c.tx, Projection::Session, &a.session_id)?)
        })?;
        if let Some(old) = replaces {
            l.pending.remove(old);
        }
        let id = a.approval_id.clone();
        let pending = Pending {
            upstream,
            material: Private::Approval(
                a,
                std::time::Instant::now() + self.ttl.to_std().expect("validated TTL"),
            ),
        };
        self.notice(l, &pending)?;
        l.pending.insert(id, pending);
        Ok(wire)
    }
    pub(super) fn approval_rpc(
        &self,
        l: &mut Live,
        peer: Peer,
        m: Method,
        p: Value,
    ) -> Result<Value> {
        let task = p["taskId"].as_str().unwrap();
        // Material can remain renewable after expiry. Retired IDs keep their last status.
        self.store.read(|db| l.owns(db, peer, task))?;
        self.expire(l)?;
        match m {
            Method::ApprovalList => self.store.read(|db| {
                let mut requests = vec![];
                for pending in l.pending.values() {
                    if let Private::Approval(a, _) = &pending.material
                        && a.task_id == task
                    {
                        let mut v = json!(a);
                        v["state"] = json!(lookup(db, &a.approval_id, task)?.state);
                        requests.push(v);
                    }
                }
                Ok(json!({"requests":requests}))
            }),
            Method::ApprovalGet => self.store.read(|db| {
                Ok(status(&lookup(
                    db,
                    p["approvalId"].as_str().unwrap(),
                    task,
                )?))
            }),
            Method::ApprovalRenew => {
                let id = p["approvalId"].as_str().unwrap();
                let a = self.store.read(|db| lookup(db, id, task))?;
                if a.state == "withdrawn" {
                    return Err(ErrorReason::RequestWithdrawn.into());
                }
                let Some(Pending {
                    upstream,
                    material: Private::Approval(live, _),
                }) = l.pending.get(id)
                else {
                    return Err(ErrorReason::Conflict.into());
                };
                if a.state != "expired" {
                    return Err(ErrorReason::Conflict.into());
                }
                self.issue(l, upstream.clone(), live.clone(), Some(id))
            }
            Method::ApprovalDecide => self.decide(l, peer, decode(p)?),
            _ => unreachable!(),
        }
    }
    fn decide(&self, l: &mut Live, peer: Peer, p: ApprovalDecideParams) -> Result<Value> {
        let params_hash = hash(&json!(p));
        let key = storage::RequestKey {
            id: &p.decision_id,
            method: "approval.decide",
            task: &p.task_id,
            approval: Some(&p.approval_id),
            params_hash: &params_hash,
        };
        let (mut a, retry) = self.store.write(|db| {
            l.owns(db, peer, &p.task_id)?;
            let prior = db.first_reply(&key)?;
            let mut a = lookup(db, &p.approval_id, &p.task_id)?;
            if prior.is_some() {
                return Ok((a, true));
            }
            if issued(db, &p.decision_id)? {
                return Err(ErrorReason::Conflict.into());
            }
            match a.state.as_str() {
                "withdrawn" => return Err(ErrorReason::RequestWithdrawn.into()),
                "expired" => return Err(ErrorReason::ApprovalExpired.into()),
                "pending" if a.expires_at <= now() => {
                    return Err(ErrorReason::ApprovalExpired.into());
                }
                "pending" => {}
                _ => return Err(ErrorReason::Conflict.into()),
            }
            if a.digest != p.digest {
                return Err(ErrorReason::DigestMismatch.into());
            }
            let pending = l
                .pending
                .get(&p.approval_id)
                .ok_or(ErrorReason::RequestWithdrawn)?;
            let Private::Approval(request, deadline) = &pending.material else {
                return Err(ErrorReason::RequestWithdrawn.into());
            };
            if *deadline <= std::time::Instant::now() {
                return Err(ErrorReason::ApprovalExpired.into());
            }
            let h = l
                .processes
                .get(&a.session)
                .ok_or(ErrorReason::RequestWithdrawn)?;
            if !h.valid_decision(&pending.upstream, request.kind, p.choice, p.answer.as_ref()) {
                return Err(ErrorReason::InvalidParams.into());
            }
            a.state = "decided".into();
            a.decision = Some(storage::Decision {
                id: p.decision_id.clone(),
                choice: json!(p.choice).as_str().unwrap().into(),
                decided_at: now(),
                delivery: "recorded".into(),
            });
            db.put_approval(&a)?;
            db.db
                .execute("UPDATE callbacks SET state='resolved' WHERE id=?", [&a.id])?;
            // A retry reconstructs delivery from the current status. The hash binds any private answer.
            db.record_reply(&key, &json!({"approvalId":a.id}))?;
            Ok((a, false))
        })?;
        if retry {
            return decision_reply(&a, &p);
        }
        let pending = l.pending.remove(&a.id).ok_or(ErrorReason::InternalError)?;
        let h = l
            .processes
            .get_mut(&a.session)
            .ok_or(ErrorReason::InternalError)?;
        let sent = h
            .reply(
                &pending.upstream,
                Reply::Decision {
                    choice: p.choice,
                    answer: p.answer.clone(),
                },
            )
            .is_ok();
        a.decision.as_mut().unwrap().delivery = if sent { "sent" } else { "unknown" }.into();
        self.commit(l, |c| {
            c.tx.put_approval(&a)?;
            let s = object(c.tx, Projection::Session, &a.session)?;
            if sent {
                c.waiting_state(s)?;
            } else {
                c.end(s, SessionState::Failed, Some("harness_lost".into()), None)?;
            }
            Ok(())
        })?;
        if !sent {
            if let Some(h) = l.processes.get_mut(&a.session) {
                h.terminate();
            }
            l.forget_session(&a.session);
        }
        decision_reply(&a, &p)
    }
    pub(super) fn answer(&self, l: &mut Live, peer: Peer, p: InputAnswerParams) -> Result<Value> {
        let s = self.store.read(|db| {
            let s: Session = object(db, Projection::Session, &p.session_id)?;
            l.owns(db, peer, &s.task_id)?;
            let state:Option<String> = db.db.query_row(
                "SELECT state FROM callbacks WHERE id=? AND session=? AND turn=? AND kind='input'",
                params![p.request_id,p.session_id,p.turn_id],|r|r.get(0)
            ).optional()?;
            match state.as_deref() {
                Some("withdrawn") => Err(ErrorReason::RequestWithdrawn.into()),
                Some("live") => Ok(s),
                _ => Err(ErrorReason::NotFound.into()),
            }
        })?;
        let Some(Pending {
            upstream,
            material: Private::Input(q),
        }) = l.pending.get(&p.request_id)
        else {
            return Err(ErrorReason::NotFound.into());
        };
        q.validate_answer_ids(&p.answers)?;
        let h = l
            .processes
            .get_mut(&s.session_id)
            .ok_or(ErrorReason::InternalError)?;
        if h.reply(upstream, Reply::Input(p.answers)).is_err() {
            self.fail(l, &s.session_id, "harness_lost")?;
            return Err(ErrorReason::RequestWithdrawn.into());
        }
        self.commit(l, |c| {
            c.tx.db.execute(
                "UPDATE callbacks SET state='resolved' WHERE id=?",
                [&p.request_id],
            )?;
            c.waiting_state(s)
        })?;
        l.pending.remove(&p.request_id);
        Ok(json!({}))
    }
}
