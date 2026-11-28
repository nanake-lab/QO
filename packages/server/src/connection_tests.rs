use super::*;

#[test]
fn params_are_checked_before_signing_a_link_or_dispatching_a_task() {
    let service = Service {
        assets: Assets::Directory(PathBuf::new()),
        backend: Arc::new(crate::backend::Bootstrap),
        next_peer: Arc::new(AtomicU64::new(1)),
        authority: String::from("127.0.0.1:47261"),
        origin: String::from("http://127.0.0.1:47261"),
        native: String::new(),
        desktop: String::new(),
        browser: Arc::new(Mutex::new(BrowserSessions::new(Duration::from_secs(60)))),
    };
    let mut ready = false;
    let mut call = |name, params| {
        dispatch(
            &mut ready,
            name,
            params,
            Peer {
                id: 1,
                role: Role::Desktop,
            },
            &service,
        )
    };
    assert_eq!(
        call("state.get", json!({})),
        Err(ErrorReason::InitializeRequired.into())
    );
    call("initialize", json!({"versions":["qo.v1"]})).unwrap();
    assert_eq!(
        call("ui.loginLink", json!({"token":"extra"})),
        Err(ErrorReason::InvalidParams.into())
    );
    assert_eq!(
        call("ui.loginLink", Value::Null),
        Err(ErrorReason::InvalidParams.into())
    );
    assert_eq!(
        call("task.close", json!({"taskId":"bad"})),
        Err(ErrorReason::InvalidParams.into())
    );
    assert_eq!(
        call(
            "task.close",
            json!({"taskId":"00000000-0000-4000-8000-000000000001"})
        ),
        Err(ErrorReason::Forbidden.into())
    );
    assert!(qo_protocol::validate_output(
        "ui.loginLink",
        &call("ui.loginLink", json!({})).unwrap()
    ));
}
