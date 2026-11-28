use super::*;
use crate::backend::Outbound;
use std::time::Duration;

struct Factory(mpsc::Sender<EventSink>, mpsc::Sender<(String, Reply)>);
struct Process(mpsc::Sender<(String, Reply)>);

#[test]
fn an_overflowing_subscriber_can_resume_from_its_applied_cursor() {
    let root = tempfile::tempdir().unwrap();
    let settings = Settings::default();
    let service = ServiceBackend::new(
        Store::open_in(root.path(), &settings).unwrap(),
        &settings,
        root.path().join("workspaces"),
        None,
    )
    .unwrap();
    let peer = Peer {
        id: 1,
        role: Role::Browser,
    };
    let connect = |peer, capacity| {
        let (tx, rx) = tokio::sync::mpsc::channel(capacity);
        let (overflow, lagged) = tokio::sync::watch::channel(false);
        service.connected(peer, Notifications { tx, overflow });
        (rx, lagged)
    };
    let (_slow, lagged) = connect(peer, 1);
    service
        .call(peer, Method::EventsSubscribe, json!({"afterCursor":"0"}))
        .unwrap();
    let task = Uuid::new_v4().to_string();
    service
        .call(
            peer,
            Method::TaskCreate,
            json!({"taskId":task,"name":"overflow"}),
        )
        .unwrap();
    assert!(*lagged.borrow());
    service.disconnected(peer);
    let resumed = Peer { id: 2, ..peer };
    let (mut rx, lagged) = connect(resumed, 1);
    service
        .call(resumed, Method::EventsSubscribe, json!({"afterCursor":"0"}))
        .unwrap();
    let Outbound::Replay(mut replay) = rx.try_recv().unwrap() else {
        panic!("missing replay")
    };
    let page = replay.next_page().unwrap();
    assert_eq!(page.len(), 1);
    assert_eq!(page[0]["cursor"], "1");
    assert_eq!(page[0]["taskId"], task);
    assert!(!*lagged.borrow());
}
impl HarnessFactory for Factory {
    fn name(&self) -> &str {
        "test"
    }
    fn start(&self, _: &Task, _: &SessionOpenParams, sink: EventSink) -> io::Result<Started> {
        self.0.send(sink).unwrap();
        Ok(Started {
            process: Box::new(Process(self.1.clone())),
            version: "1".into(),
            model: None,
            reasoning_effort: None,
            service_tier: None,
        })
    }
}
impl Harness for Process {
    fn valid_decision(
        &self,
        _: &str,
        _: ApprovalKind,
        choice: Choice,
        answer: Option<&serde_json::Map<String, Value>>,
    ) -> bool {
        choice == Choice::Deny && answer.is_none()
    }
    fn start_turn(&mut self, _: &str, _: &str) -> io::Result<()> {
        Ok(())
    }
    fn append(&mut self, _: &str, _: &str) -> io::Result<()> {
        Ok(())
    }
    fn interrupt(&mut self, _: &str) -> io::Result<()> {
        Ok(())
    }
    fn finish(&mut self) -> io::Result<()> {
        Ok(())
    }
    fn terminate(&mut self) {}
    fn reply(&mut self, callback: &str, reply: Reply) -> io::Result<()> {
        self.0
            .send((callback.to_owned(), reply))
            .map_err(|_| io::Error::other("lost"))
    }
}

#[tokio::test]
async fn withdrawal_reveals_waiting_input_and_secret_is_delivered_only_to_its_callback() {
    let dir = tempfile::tempdir().unwrap();
    let settings = Settings::default();
    let (started, process) = mpsc::channel();
    let (replies, received) = mpsc::channel();
    let backend = ServiceBackend::new(
        Store::open_in(dir.path(), &settings).unwrap(),
        &settings,
        dir.path().join("workspaces"),
        Some(Box::new(Factory(started, replies))),
    )
    .unwrap();
    let owner = Peer {
        id: 1,
        role: Role::External,
    };
    let (tx, mut private) = tokio::sync::mpsc::channel(8);
    backend.connected(
        owner,
        Notifications {
            tx,
            overflow: tokio::sync::watch::channel(false).0,
        },
    );
    let task = Uuid::new_v4().to_string();
    let session = Uuid::new_v4().to_string();
    let turn = Uuid::new_v4().to_string();
    let owner_key = "ba".repeat(32);
    let call = |method, args| backend.call(owner, method, args).unwrap();
    call(
        Method::TaskCreate,
        json!({"taskId":task,"name":"test","ownerKey":owner_key}),
    );
    call(
        Method::SessionOpen,
        json!({"taskId":task,"sessionId":session}),
    );
    call(
        Method::TurnStart,
        json!({"sessionId":session,"turnId":turn,"input":"test"}),
    );
    let sink = process.recv().unwrap();
    let questions = serde_json::from_value(
        json!([{"id":"q","header":"note","question":"note?","allowOther":true,"secret":true}]),
    )
    .unwrap();
    sink.send(HarnessEvent::Input {
        callback: "input-id".into(),
        turn: turn.clone(),
        questions,
    })
    .unwrap();
    sink.send(HarnessEvent::Approval {
        callback: "approval-id".into(),
        turn: Some(turn.clone()),
        item: None,
        material: Material {
            kind: ApprovalKind::Permissions,
            raw: RawRequest {
                method: "private-method-marker".into(),
                params: Default::default(),
                context: None,
            },
            summary: "private-summary-marker".into(),
        },
    })
    .unwrap();
    async fn notice(rx: &mut tokio::sync::mpsc::Receiver<Outbound>, method: &str) -> Value {
        let Some(Outbound::Frames(frames)) =
            tokio::time::timeout(Duration::from_secs(5), rx.recv())
                .await
                .unwrap()
        else {
            panic!("missing notice")
        };
        assert_eq!(frames[0]["method"], method);
        frames[0]["params"].clone()
    }
    let input = notice(&mut private, "input.requested").await;
    let approval = notice(&mut private, "approval.requested").await;
    assert_eq!(
        call(Method::StateGet, json!({}))["sessions"][0]["state"],
        "waiting_approval"
    );
    sink.send(HarnessEvent::Withdrawn {
        callback: "approval-id".into(),
    })
    .unwrap();
    // This notification fences withdrawal on the same per-process channel without sleeps.
    sink.send(HarnessEvent::Input {
        callback: "second-input".into(),
        turn: turn.clone(),
        questions: serde_json::from_value(input["questions"].clone()).unwrap(),
    })
    .unwrap();
    let second = notice(&mut private, "input.requested").await;
    assert_eq!(
        call(
            Method::ApprovalGet,
            json!({"taskId":task,"approvalId":approval["approvalId"]})
        )["state"],
        "withdrawn"
    );
    assert_eq!(
        call(Method::StateGet, json!({}))["sessions"][0]["state"],
        "waiting_input"
    );
    let secret = "secret-input-marker";
    let answer = |request: &Value| json!({"sessionId":session,"turnId":turn,"requestId":request["requestId"],"answers":{"q":{"answers":[secret]}}});
    let mut invalid = answer(&input);
    invalid["answers"]["missing"] = json!({"answers":["invalid"]});
    assert_eq!(
        backend.call(owner, Method::InputAnswer, invalid),
        Err(ErrorReason::InvalidParams.into())
    );
    assert!(received.try_recv().is_err());
    for (request, callback) in [(&input, "input-id"), (&second, "second-input")] {
        call(Method::InputAnswer, answer(request));
        let (key, Reply::Input(answers)) = received.try_recv().unwrap() else {
            panic!("missing answers")
        };
        assert_eq!(key, callback);
        assert_eq!(answers["q"].answers, [secret]);
    }
    assert_eq!(
        call(Method::StateGet, json!({}))["sessions"][0]["state"],
        "running"
    );
    for file in std::fs::read_dir(dir.path()).unwrap() {
        let path = file.unwrap().path();
        if path.is_file() {
            let bytes = std::fs::read(path).unwrap();
            for marker in [
                secret,
                "private-method-marker",
                "private-summary-marker",
                &owner_key,
            ] {
                assert!(
                    !bytes
                        .windows(marker.len())
                        .any(|part| part == marker.as_bytes()),
                    "private data on disk"
                );
            }
        }
    }
}
