use crate::{
    scenario::{Approval, Operation},
    state::{State, id, now},
};
use chrono::{SecondsFormat, Utc};
use qo_protocol::*;
use qo_server::{Peer, RpcResult};
use serde_json::json;
use sha2::{Digest as _, Sha256};
use std::time::Instant;

impl State {
    pub fn expire(&mut self) {
        let clock = Instant::now();
        for a in self.approvals.values_mut() {
            if a.request.state == ApprovalState::Pending && clock >= a.deadline {
                a.request.state = ApprovalState::Expired;
            }
        }
    }
    pub fn issue(&mut self, t: &Turn, item: Id, operation: Operation) -> Id {
        let task = self.sessions[&t.session_id].task_id.clone();
        let (kind, method, params, summary) = match &operation {
            Operation::Command { command, .. } => (
                ApprovalKind::CommandExecution,
                "item/commandExecution/requestApproval",
                json!({"command":command}),
                command.clone(),
            ),
            Operation::File { path, diff } => (
                ApprovalKind::FileChange,
                "item/fileChange/requestApproval",
                json!({"path":path,"diff":diff}),
                format!("修改 {path}\n{diff}"),
            ),
        };
        let aid = id();
        let raw = RawRequest {
            method: method.into(),
            params: params.as_object().unwrap().clone(),
            context: None,
        };
        let expires = (Utc::now() + self.ttl).to_rfc3339_opts(SecondsFormat::Millis, true);
        let mut material = json!({"approvalId":aid,"taskId":task,"sessionId":t.session_id,"turnId":t.turn_id,
            "itemId":item,"kind":kind,"raw":raw,"summary":summary,"expiresAt":expires});
        // The bundled scripts use only ASCII property names and string leaves in frozen material.
        // Sorted keys and compact UTF-8 JSON therefore give RFC 8785 encoding for this subset.
        material.sort_all_objects();
        let digest = format!(
            "{:x}",
            Sha256::digest(serde_json::to_vec(&material).unwrap())
        );
        let request = ApprovalRequest {
            approval_id: aid.clone(),
            task_id: task.clone(),
            session_id: t.session_id.clone(),
            turn_id: Some(t.turn_id.clone()),
            item_id: Some(item),
            kind,
            raw,
            summary,
            digest,
            expires_at: expires,
            state: ApprovalState::Pending,
        };
        if self.sessions[&t.session_id].state != SessionState::WaitingApproval {
            self.session_state(t, SessionState::WaitingApproval);
        }
        self.private(&task, "approval.requested", &json!(request));
        self.approvals.insert(
            aid.clone(),
            Approval {
                request,
                operation,
                deadline: Instant::now() + self.ttl.to_std().unwrap(),
            },
        );
        aid
    }
    pub fn list(&self, peer: Peer, task: &str) -> RpcResult {
        self.require_owner(peer, task)?;
        let requests: Vec<_> = self
            .approvals
            .values()
            .filter(|a| {
                a.request.task_id == task
                    && self
                        .play
                        .get(a.request.turn_id.as_ref().unwrap())
                        .and_then(|p| p.waiting.as_ref())
                        == Some(&a.request.approval_id)
            })
            .map(|a| &a.request)
            .collect();
        Ok(json!({"requests":requests}))
    }
    pub fn renew(&mut self, peer: Peer, p: ApprovalRenewParams) -> RpcResult {
        self.require_owner(peer, &p.task_id)?;
        let a = self
            .approvals
            .get(&p.approval_id)
            .filter(|a| a.request.task_id == p.task_id)
            .ok_or(ErrorReason::NotFound)?
            .clone();
        if a.request.state == ApprovalState::Withdrawn {
            return Err(ErrorReason::RequestWithdrawn.into());
        }
        let tid = a.request.turn_id.as_ref().unwrap();
        if a.request.state != ApprovalState::Expired
            || self.play.get(tid).and_then(|s| s.waiting.as_ref()) != Some(&p.approval_id)
        {
            return Err(ErrorReason::Conflict.into());
        }
        let t = self.turns[tid].clone();
        let new_id = self.issue(&t, a.request.item_id.unwrap(), a.operation);
        self.play.get_mut(tid).unwrap().waiting = Some(new_id.clone());
        Ok(json!(self.approvals[&new_id].request))
    }
    pub fn decide(&mut self, peer: Peer, p: ApprovalDecideParams) -> RpcResult {
        self.require_owner(peer, &p.task_id)?;
        let submitted = json!(p);
        if let Some((prior, reply)) = self.decisions.get(&p.decision_id) {
            return if prior == &submitted {
                Ok(reply.clone())
            } else {
                Err(ErrorReason::Conflict.into())
            };
        }
        let a = self
            .approvals
            .get_mut(&p.approval_id)
            .filter(|a| a.request.task_id == p.task_id)
            .ok_or(ErrorReason::NotFound)?;
        match a.request.state {
            ApprovalState::Withdrawn => return Err(ErrorReason::RequestWithdrawn.into()),
            ApprovalState::Expired => return Err(ErrorReason::ApprovalExpired.into()),
            ApprovalState::Decided => return Err(ErrorReason::Conflict.into()),
            ApprovalState::Pending => {}
        }
        if p.digest != a.request.digest {
            return Err(ErrorReason::DigestMismatch.into());
        }
        if p.answer.is_some() {
            return Err(ErrorReason::InvalidParams.into());
        } // Command and file scenarios have no form.
        let t = self.turns[a.request.turn_id.as_ref().unwrap()].clone();
        let item = a.request.item_id.clone().unwrap();
        let op = a.operation.clone();
        let decision = json!(ApprovalDecision {
            decision_id: p.decision_id.clone(),
            approval_id: p.approval_id,
            digest: p.digest,
            choice: p.choice,
            answer: None,
            decided_at: now(),
            delivery: Delivery::Sent
        });
        a.request.state = ApprovalState::Decided;
        self.decisions
            .insert(p.decision_id, (submitted, decision.clone()));
        match p.choice {
            Choice::AllowOnce => self.complete_operation(&t, &item, &op),
            Choice::Deny => self.end_item(&t, &item, ItemState::Interrupted),
        }
        self.resume(&t);
        Ok(decision)
    }
    pub fn answer(&mut self, peer: Peer, p: InputAnswerParams) -> RpcResult {
        let s = self
            .sessions
            .get(&p.session_id)
            .ok_or(ErrorReason::NotFound)?;
        self.require_owner(peer, &s.task_id)?;
        if self
            .withdrawn_inputs
            .get(&p.request_id)
            .is_some_and(|(s, t)| s == &p.session_id && t == &p.turn_id)
        {
            return Err(ErrorReason::RequestWithdrawn.into());
        }
        let q = self
            .inputs
            .get(&p.request_id)
            .filter(|q| q.turn_id == p.turn_id && q.session_id == p.session_id)
            .ok_or(ErrorReason::NotFound)?;
        q.validate_answer_ids(&p.answers)?;
        // Consume the private input without copying any answer into public projections or logs.
        self.inputs.remove(&p.request_id);
        let t = self.turns[&p.turn_id].clone();
        self.resume(&t);
        Ok(json!({}))
    }
}
