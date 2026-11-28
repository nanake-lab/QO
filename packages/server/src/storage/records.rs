use super::{Error, Result, View, Write};
use rusqlite::{OptionalExtension, params};
use serde_json::Value;

#[derive(Debug, PartialEq)]
pub struct Owner {
    pub key_hash: Option<[u8; 32]>,
    pub creation_hash: [u8; 32],
}

/// The same client ID must always name the same operation, task and input.
pub struct RequestKey<'a> {
    pub id: &'a str,
    pub method: &'a str,
    pub task: &'a str,
    pub approval: Option<&'a str>,
    pub params_hash: &'a [u8; 32],
}

#[derive(Debug, PartialEq)]
pub struct Approval {
    pub id: String,
    pub task: String,
    pub session: String,
    pub kind: String,
    pub state: String,
    pub expires_at: String,
    pub digest: String,
    pub decision: Option<Decision>,
}

#[derive(Debug, PartialEq)]
pub struct Decision {
    pub id: String,
    pub choice: String,
    pub decided_at: String,
    pub delivery: String,
}

impl View<'_> {
    pub fn owner(&self, task: &str) -> Result<Option<Owner>> {
        Ok(self
            .db
            .query_row(
                "SELECT key_hash,creation_hash FROM owners WHERE task=?",
                [task],
                |r| {
                    Ok(Owner {
                        key_hash: r.get(0)?,
                        creation_hash: r.get(1)?,
                    })
                },
            )
            .optional()?)
    }

    /// Look up inside the write transaction before doing any repeatable operation.
    pub fn first_reply(&self, key: &RequestKey<'_>) -> Result<Option<Value>> {
        let row = self
            .db
            .query_row(
                "SELECT method=?2 AND task=?3 AND approval IS ?4 AND params_hash=?5, reply
            FROM receipts WHERE id=?1",
                params![key.id, key.method, key.task, key.approval, key.params_hash],
                |r| Ok((r.get::<_, bool>(0)?, r.get(1)?)),
            )
            .optional()?;
        match row {
            Some((false, _)) => Err(Error::Conflict),
            Some((true, reply)) => Ok(Some(reply)),
            None => Ok(None),
        }
    }

    pub fn approval(&self, id: &str) -> Result<Option<Approval>> {
        Ok(self
            .db
            .query_row(
                "SELECT id, task, session, kind, state, expires_at, digest,
            decision_id, choice, decided_at, delivery FROM approvals WHERE id=?",
                [id],
                |r| {
                    let decision = match r.get(7)? {
                        None => None,
                        Some(id) => Some(Decision {
                            id,
                            choice: r.get(8)?,
                            decided_at: r.get(9)?,
                            delivery: r.get(10)?,
                        }),
                    };
                    Ok(Approval {
                        id: r.get(0)?,
                        task: r.get(1)?,
                        session: r.get(2)?,
                        kind: r.get(3)?,
                        state: r.get(4)?,
                        expires_at: r.get(5)?,
                        digest: r.get(6)?,
                        decision,
                    })
                },
            )
            .optional()?)
    }
}

impl Write<'_> {
    pub fn insert_owner(&self, task: &str, owner: &Owner) -> Result<()> {
        self.db.execute(
            "INSERT INTO owners (task,key_hash,creation_hash) VALUES (?,?,?)",
            params![task, owner.key_hash, owner.creation_hash],
        )?;
        Ok(())
    }

    pub fn record_reply(&self, key: &RequestKey<'_>, reply: &Value) -> Result<()> {
        let added = self.db.execute(
            "INSERT INTO receipts (id,method,task,approval,params_hash,reply)
            VALUES (?,?,?,?,?,?) ON CONFLICT(id) DO NOTHING",
            params![
                key.id,
                key.method,
                key.task,
                key.approval,
                key.params_hash,
                reply.to_string()
            ],
        )?;
        if added == 0 {
            return Err(Error::Conflict);
        }
        Ok(())
    }

    pub fn put_approval(&self, a: &Approval) -> Result<()> {
        let decision = a.decision.as_ref();
        let changed = self.db.execute(
            "INSERT INTO approvals (id, task, session, kind, state, expires_at, digest,
                                   decision_id, choice, decided_at, delivery)
             VALUES (?,?,?,?,?,?,?,?,?,?,?)
             ON CONFLICT(id) DO UPDATE SET state=excluded.state, decision_id=excluded.decision_id,
                 choice=excluded.choice, decided_at=excluded.decided_at, delivery=excluded.delivery
             WHERE task=excluded.task AND session=excluded.session AND kind=excluded.kind
                 AND expires_at=excluded.expires_at AND digest=excluded.digest",
            params![
                a.id,
                a.task,
                a.session,
                a.kind,
                a.state,
                a.expires_at,
                a.digest,
                decision.map(|d| &d.id),
                decision.map(|d| &d.choice),
                decision.map(|d| &d.decided_at),
                decision.map(|d| &d.delivery)
            ],
        )?;
        if changed == 0 {
            return Err(Error::Conflict);
        }
        Ok(())
    }
}
