#[cfg(not(feature = "test-harness"))]
use qo_conformance::Real as Target;
#[cfg(feature = "test-harness")]
use qo_conformance::ScriptedReal as Target;
const TARGET: Target = Target(env!("CARGO_BIN_EXE_qo-server"));
mod connection {
    use super::TARGET;
    #[test]
    fn headers_and_credentials() {
        qo_conformance::connection::headers_and_credentials(&TARGET);
    }
    #[test]
    fn negotiation_and_roles() {
        qo_conformance::connection::negotiation_and_roles(&TARGET);
    }
    #[test]
    fn restart_and_single_use() {
        qo_conformance::connection::restart_and_single_use(&TARGET);
    }
    #[test]
    fn link_expiry() {
        qo_conformance::connection::link_expiry(&TARGET);
    }
}
#[cfg(feature = "test-harness")]
mod tasks {
    use super::TARGET;
    #[test]
    fn ownership_and_close() {
        qo_conformance::tasks::ownership_and_close(&TARGET);
    }
    #[test]
    fn idempotent_writes() {
        qo_conformance::tasks::idempotent_writes(&TARGET);
    }
    #[test]
    fn effective_model() {
        qo_conformance::tasks::effective_model(&TARGET);
    }
}
#[cfg(feature = "test-harness")]
mod events {
    use super::TARGET;
    #[test]
    fn replay_and_live() {
        qo_conformance::events::replay_and_live(&TARGET);
    }
    #[test]
    fn retention_and_snapshot() {
        qo_conformance::events::retention_and_snapshot(&TARGET);
    }
}
#[cfg(feature = "test-harness")]
mod approvals {
    use super::TARGET;
    #[test]
    fn private_decisions_and_single_use() {
        qo_conformance::approvals::private_decisions_and_single_use(&TARGET);
    }
    #[test]
    fn expiry_and_renewal() {
        qo_conformance::approvals::expiry_and_renewal(&TARGET);
    }
}

#[cfg(feature = "test-harness")]
#[test]
fn restart_reconciliation() {
    qo_conformance::recovery::restart(&TARGET);
}

#[cfg(feature = "test-harness")]
#[test]
fn storage_error_exits_process_and_reconciles_on_restart() {
    let diagnostic = qo_conformance::recovery::storage_failure(&TARGET, |root| {
        let state = root.join(if cfg!(windows) {
            "QO/state"
        } else {
            "state/qo"
        });
        let db = rusqlite::Connection::open(state.join("qo.sqlite3")).unwrap();
        // Fail the first append at event insertion, after its projection changed inside the transaction.
        db.execute_batch(
            "CREATE TRIGGER fail_append BEFORE INSERT ON events
            WHEN NEW.kind = 'turn.changed' AND NEW.body ->> '$.payload.state' = 'running'
            BEGIN SELECT RAISE(ABORT, 'private-fault-marker'); END;",
        )
        .unwrap();
    });
    assert!(
        diagnostic.contains("turn.append") && diagnostic.contains("SQLite ConstraintViolation")
    );
    assert!(!diagnostic.contains("private-fault-marker"));
}
