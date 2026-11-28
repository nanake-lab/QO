use super::*;
use serde_json::json;
use tempfile::tempdir;

fn key() -> RequestKey<'static> {
    RequestKey {
        id: "client",
        method: "task.create",
        task: "t",
        approval: None,
        params_hash: &[3; 32],
    }
}

fn event(at: &str) -> Value {
    json!({"at":at,"taskId":"t","kind":"item.delta","payload":{"itemId":"i","delta":"中文"}})
}

fn now(db: &View<'_>) -> Result<String> {
    Ok(db
        .db
        .query_row("SELECT strftime('%Y-%m-%dT%H:%M:%SZ','now')", [], |r| {
            r.get(0)
        })?)
}

fn task(state: &str) -> Value {
    json!({"taskId":"t","state":state,"name":"example"})
}

#[test]
fn rollback_includes_projection_receipt_and_cursor() -> Result<()> {
    let dir = tempdir()?;
    let store = Store::open_in(dir.path(), &Settings::default())?;
    let result = store.write(|w| {
        w.put_projection(Projection::Task, &task("open"))?;
        w.record_reply(&key(), &task("open"))?;
        assert_eq!(w.append_event(event(&now(w)?))?, 1);
        // The failing SQL operation is last, after all three successful writes.
        w.put_projection(Projection::Session, &json!({}))
    });
    assert!(matches!(result, Err(Error::Database(_))));
    store.write(|w| {
        assert!(w.snapshot()?.tasks.is_empty());
        assert!(w.first_reply(&key())?.is_none());
        assert!(w.replay(0, 0)?.is_empty());
        assert_eq!(w.append_event(event(&now(w)?))?, 1);
        assert_eq!(w.append_event(event(&now(w)?))?, 2);
        Ok(())
    })?;
    store.read(|r| {
        assert_eq!(r.high_water()?, 2);
        assert_eq!(r.replay(0, 1)?[0]["cursor"], "1");
        assert_eq!(r.replay(1, 2)?[0]["cursor"], "2");
        assert!(matches!(r.replay(3, 3), Err(Error::InvalidCursor)));
        Ok(())
    })
}

#[test]
fn restart_preserves_private_indexes_and_original_replies() -> Result<()> {
    let dir = tempdir()?;
    let settings = Settings::default();
    let store = Store::open_in(dir.path(), &settings)?;
    let owner = Owner {
        key_hash: Some([1; 32]),
        creation_hash: [2; 32],
    };
    let decision = RequestKey {
        id: "decision",
        method: "approval.decide",
        approval: Some("a"),
        ..key()
    };
    let mut approval = Approval {
        id: "a".into(),
        task: "t".into(),
        session: "s".into(),
        kind: "command".into(),
        state: "pending".into(),
        expires_at: "2026-11-28T07:00:00Z".into(),
        digest: "ab".repeat(32),
        decision: None,
    };
    store.write(|w| {
        w.insert_owner("t", &owner)?;
        w.record_reply(&key(), &task("open"))?;
        w.put_approval(&approval)?;
        assert_eq!(w.approval("a")?.as_ref(), Some(&approval));
        w.append_event(event(&now(w)?))?;
        Ok(())
    })?;
    approval.state = "decided".into();
    approval.decision = Some(Decision {
        id: "decision".into(),
        choice: "deny".into(),
        decided_at: "2026-11-28T06:59:00Z".into(),
        delivery: "sent".into(),
    });
    let reply = json!({"delivery":"sent"});
    store.write(|w| {
        w.put_approval(&approval)?;
        w.record_reply(&decision, &reply)
    })?;
    drop(store);
    let reopened = Store::open_in(dir.path(), &settings)?;
    reopened.read(|r| {
        assert_eq!(r.high_water()?, 1);
        assert_eq!(r.owner("t")?, Some(owner));
        assert_eq!(r.approval("a")?, Some(approval));
        assert_eq!(r.first_reply(&key())?, Some(task("open")));
        assert_eq!(r.first_reply(&decision)?, Some(reply));
        Ok(())
    })
}

#[test]
fn client_id_binds_method_task_approval_and_parameters() -> Result<()> {
    let dir = tempdir()?;
    let store = Store::open_in(dir.path(), &Settings::default())?;
    store.write(|w| w.record_reply(&key(), &json!({"first":true})))?;
    for other in [
        RequestKey {
            method: "session.open",
            ..key()
        },
        RequestKey {
            task: "another",
            ..key()
        },
        RequestKey {
            params_hash: &[4; 32],
            ..key()
        },
        RequestKey {
            method: "approval.decide",
            approval: Some("a"),
            ..key()
        },
    ] {
        assert!(matches!(
            store.read(|r| r.first_reply(&other)),
            Err(Error::Conflict)
        ));
        assert!(matches!(
            store.write(|w| w.record_reply(&other, &json!({}))),
            Err(Error::Conflict)
        ));
    }
    Ok(())
}

#[test]
fn time_limit_discards_contiguous_prefix_even_after_clock_moves_back() -> Result<()> {
    let dir = tempdir()?;
    let store = Store::open_in(dir.path(), &Settings::default())?;
    store.write(|w| {
        let current = now(w)?;
        for at in [&current, "2000-01-01T00:00:00Z", &current, &current] {
            w.append_event(event(at))?;
        }
        Ok(())
    })?;
    store.read(|r| {
        assert!(matches!(
            r.replay(1, 4),
            Err(Error::CursorExpired { oldest: 3, high: 4 })
        ));
        let events = r.replay(2, 4)?;
        assert_eq!(events.len(), 2);
        assert_eq!(events[0]["cursor"], "3");
        assert_eq!(events[1]["cursor"], "4");
        Ok(())
    })
}

#[test]
fn bytes_count_complete_utf8_events_and_empty_log_keeps_high_water() -> Result<()> {
    let dir = tempdir()?;
    let mut settings = Settings::default();
    let store = Store::open_in(dir.path(), &settings)?;
    let at = store.read(now)?;
    store.write(|w| {
        for _ in 0..3 {
            w.append_event(event(&at))?;
        }
        Ok(())
    })?;
    let last = store.read(|r| r.replay(2, 3))?.pop().unwrap();
    drop(store);
    settings.event_retention_bytes = last.to_string().len() as u64;
    let store = Store::open_in(dir.path(), &settings)?;
    store.read(|r| {
        assert!(matches!(
            r.replay(1, 3),
            Err(Error::CursorExpired { oldest: 3, high: 3 })
        ));
        assert_eq!(r.replay(2, 3)?, vec![last]);
        Ok(())
    })?;
    drop(store);
    settings.event_retention_bytes -= 1;
    let store = Store::open_in(dir.path(), &settings)?;
    store.read(|r| {
        assert!(matches!(
            r.replay(2, 3),
            Err(Error::CursorExpired { oldest: 4, high: 3 })
        ));
        assert!(r.replay(3, 3)?.is_empty());
        Ok(())
    })?;
    assert_eq!(store.write(|w| w.append_event(event(&at)))?, 4);
    Ok(())
}

#[test]
fn readers_keep_one_commit_while_writer_updates_and_prunes() -> Result<()> {
    let dir = tempdir()?;
    let store = Store::open_in(dir.path(), &Settings::default())?;
    store.write(|w| {
        w.put_projection(Projection::Task, &task("open"))?;
        w.append_event(event(&now(w)?))?;
        Ok(())
    })?;
    store.read(|reader| {
        assert_eq!(reader.high_water()?, 1);
        std::thread::scope(|scope| {
            scope
                .spawn(|| {
                    store.write(|w| {
                        w.put_projection(Projection::Task, &task("closed"))?;
                        let halfway = store.read(|r| r.snapshot())?;
                        assert_eq!(halfway.tasks, vec![task("open")]);
                        assert_eq!(halfway.cursor, 1);
                        w.append_event(event("2000-01-01T00:00:00Z"))?;
                        Ok(())
                    })
                })
                .join()
                .unwrap()
        })?;
        let old = reader.snapshot()?;
        assert_eq!(old.tasks, vec![task("open")]);
        assert_eq!(old.cursor, 1);
        assert_eq!(reader.replay(0, 1)?.len(), 1);
        Ok(())
    })?;
    store.read(|r| {
        let new = r.snapshot()?;
        assert_eq!(new.tasks, vec![task("closed")]);
        assert_eq!(new.cursor, 2);
        assert!(matches!(
            r.replay(0, 2),
            Err(Error::CursorExpired { oldest: 3, high: 2 })
        ));
        Ok(())
    })
}

#[test]
fn usage_dimensions_replace_only_the_matching_snapshot() -> Result<()> {
    let dir = tempdir()?;
    let store = Store::open_in(dir.path(), &Settings::default())?;
    let base = json!({"source":"thread","sessionId":"s","counts":{"totalTokens":"0"}});
    store.write(|w| {
        w.put_projection(Projection::Usage, &base)?;
        for (field, value) in [
            ("source", "group"),
            ("sessionId", "s2"),
            ("turnId", "t"),
            ("model", ""),
            ("reasoningEffort", "low"),
            ("serviceTier", "fast"),
        ] {
            let mut group = base.clone();
            group[field] = json!(value);
            w.put_projection(Projection::Usage, &group)?;
        }
        let mut next = base.clone();
        next["counts"]["totalTokens"] = json!("10");
        w.put_projection(Projection::Usage, &next)?;
        for (kind, object) in [
            (
                Projection::Session,
                json!({"sessionId":"s","taskId":"t","state":"idle","extra":"preserved"}),
            ),
            (
                Projection::Turn,
                json!({"turnId":"u","sessionId":"s","state":"running"}),
            ),
            (
                Projection::Item,
                json!({"itemId":"i","turnId":"u","state":"running"}),
            ),
        ] {
            w.put_projection(kind, &object)?;
        }
        let snap = w.snapshot()?;
        assert_eq!(snap.usage.len(), 7);
        assert!(snap.usage.contains(&next));
        assert!(!snap.usage.contains(&base));
        assert_eq!(snap.sessions[0]["extra"], "preserved");
        assert_eq!(snap.turns.len(), 1);
        assert_eq!(snap.items.len(), 1);
        Ok(())
    })
}

#[test]
fn refuses_newer_schema_without_modifying_it() -> Result<()> {
    let dir = tempdir()?;
    let db = Connection::open(dir.path().join("qo.sqlite3"))?;
    db.execute_batch("PRAGMA user_version=3")?;
    assert!(matches!(
        Store::open_in(dir.path(), &Settings::default()),
        Err(Error::Version(3))
    ));
    assert_eq!(
        db.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))?,
        3
    );
    Ok(())
}

#[test]
fn retention_settings_are_optional_and_strict() {
    let defaults: Settings = serde_json::from_value(json!({"port":12345})).unwrap();
    assert_eq!(defaults.event_retention_days, 7);
    assert_eq!(defaults.event_retention_bytes, 256 * 1024 * 1024);
    let custom: Settings =
        serde_json::from_value(json!({"eventRetentionDays":0,"eventRetentionBytes":42})).unwrap();
    assert_eq!(custom.event_retention_days, 0);
    assert_eq!(custom.event_retention_bytes, 42);
    assert!(serde_json::from_value::<Settings>(json!({"retentionBytes":1})).is_err());
}

#[test]
fn replay_keeps_its_pages_while_live_writes_prune_history() -> Result<()> {
    let dir = tempdir()?;
    let store = Store::open_in(dir.path(), &Settings::default())?;
    let at = store.read(now)?;
    store.write(|tx| {
        for _ in 0..150 {
            tx.append_event(event(&at))?;
        }
        Ok(())
    })?;
    let mut replay = store.begin_replay(0)?;
    let mut history = replay.next_page()?;
    assert!(!history.is_empty() && history.len() < 150);
    store.write(|tx| {
        tx.append_event(event("2000-01-01T00:00:00Z"))?;
        tx.append_event(event(&at))?;
        Ok(())
    })?;
    assert!(matches!(
        store.begin_replay(0),
        Err(Error::CursorExpired {
            oldest: 152,
            high: 152
        })
    ));
    loop {
        let page = replay.next_page()?;
        if page.is_empty() {
            break;
        }
        history.extend(page);
    }
    assert_eq!(history.len(), 150);
    for (i, e) in history.iter().enumerate() {
        assert_eq!(e["cursor"], (i + 1).to_string());
    }
    Ok(())
}
