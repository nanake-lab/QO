use super::*;
use qo_protocol::TurnState;

#[test]
fn effective_security_settings_are_all_verified() {
    let cwd = env::current_dir().unwrap();
    let base = json!({"approvalPolicy":"untrusted","approvalsReviewer":"user","sandbox":{"type":"readOnly"},"cwd":cwd});
    verify(&base, &cwd).unwrap();
    for (field, wrong) in [
        ("approvalsReviewer", json!("auto_review")),
        ("approvalPolicy", json!("never")),
        ("sandbox", json!({"type":"workspaceWrite"})),
        ("sandbox", json!({"type":"readOnly","networkAccess":true})),
        ("cwd", json!(cwd.parent().unwrap())),
    ] {
        let mut diff = base.clone();
        diff[field] = wrong;
        assert!(verify(&diff, &cwd).is_err(), "{field}");
    }
}

#[test]
fn recorded_round_trips() {
    let script = r#"
const fs=require('node:fs'), readline=require('node:readline'), assert=require('node:assert/strict');
const rows=fs.readFileSync(process.argv[1],'utf8').trim().split('\n').map(JSON.parse);
readline.createInterface({input:process.stdin}).on('line',line=>{
    assert.deepEqual(JSON.parse(line),rows.shift().send);
    while(rows.length && !rows[0].send){
        const row=rows.shift();
        if(row.exit) process.exit(23);
        console.log(JSON.stringify(row.recv));
    }
});
"#;
    for (name, expected) in [
        ("normal", "started delta completed total last end"),
        (
            "append",
            "started delta completed total last started delta delta completed total last end",
        ),
        ("interrupt", "interrupted"),
        ("exit", "exit"),
    ] {
        let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("fixtures")
            .join(format!("{name}.jsonl"));
        let mut child = Command::new("node");
        child
            .args(["-e", script])
            .arg(fixture)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped());
        let tree = Tree::spawn(child).unwrap();
        let (tx, rx) = mpsc::channel();
        let mut process = Process::connect(
            Arc::new(Mutex::new(tree)),
            EventSink::new("session".into(), tx),
            "session".into(),
            "wire-1".into(),
            Duration::from_secs(2),
            2,
        )
        .unwrap();
        process
            .start_turn(
                "local-turn",
                "Reply with only the word READY. Do not use tools.",
            )
            .unwrap();
        match name {
            "append" => process
                .append(
                    "local-turn",
                    "Reply only with APPENDED instead. Do not use tools.",
                )
                .unwrap(),
            "interrupt" => process.interrupt("local-turn").unwrap(),
            _ => (),
        }
        let mut seen = Vec::new();
        let mut output = String::new();
        loop {
            let (sid, event) = rx.recv_timeout(Duration::from_secs(8)).unwrap();
            assert_eq!(sid, "session");
            let label = match event {
                HarnessEvent::Item(i) => {
                    let v = json!(i);
                    assert_eq!(v["turnId"], "local-turn");
                    if v["state"] == "running" {
                        "started"
                    } else {
                        "completed"
                    }
                }
                HarnessEvent::Delta { turn, delta } => {
                    assert_eq!(turn, "local-turn");
                    output.push_str(&delta.delta);
                    "delta"
                }
                HarnessEvent::Usage(u) => {
                    assert_eq!(u.turn_id.as_deref(), Some("local-turn"));
                    assert!(u.model.is_none());
                    assert!(u.counts.total_tokens.unwrap().parse::<u64>().unwrap() > 0);
                    if u.source.ends_with(".last") {
                        "last"
                    } else {
                        assert!(u.source.ends_with(".total"));
                        "total"
                    }
                }
                HarnessEvent::TurnEnded { turn, state, .. } => {
                    assert_eq!(turn, "local-turn");
                    seen.push(if state == TurnState::Interrupted {
                        "interrupted"
                    } else {
                        assert_eq!(state, TurnState::Completed);
                        "end"
                    });
                    break;
                }
                HarnessEvent::Exited => {
                    seen.push("exit");
                    break;
                }
                _ => panic!("unexpected event"),
            };
            seen.push(label);
        }
        assert_eq!(seen.join(" "), expected);
        if name == "normal" {
            assert_eq!(output, "READY");
        }
        if name == "append" {
            assert_eq!(output, "READYAPPENDED");
        }
    }
}

#[test]
fn tree_termination_closes_a_grandchild_pipe() {
    let mut cmd = Command::new("node");
    cmd.args(["-e",r#"require('node:child_process').spawn(process.execPath,['-e',"console.log('ready');setTimeout(()=>{},12000)"],{stdio:['ignore',1,2]})"#]).stdout(Stdio::piped());
    let mut tree = Tree::spawn(cmd).unwrap();
    let out = tree.child().stdout.take().unwrap();
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let mut reader = BufReader::new(out);
        let mut line = String::new();
        reader.read_line(&mut line).unwrap();
        tx.send(line.trim().to_owned()).unwrap();
        let mut rest = String::new();
        reader.read_to_string(&mut rest).unwrap();
        let _ = tx.send(rest);
    });
    assert_eq!(rx.recv_timeout(Duration::from_secs(5)).unwrap(), "ready");
    tree.kill();
    assert_eq!(rx.recv_timeout(Duration::from_secs(3)).unwrap(), "");
}

#[test]
fn unsupported_callback_receives_an_error_and_fails_the_session() {
    let script = r#"const readline=require('node:readline'),assert=require('node:assert/strict');
const io=readline.createInterface({input:process.stdin}); setTimeout(()=>process.exit(2),5000);
io.on('line',line=>{const v=JSON.parse(line);
if(v.method==='turn/start'){console.log(JSON.stringify({id:v.id,result:{turn:{id:'wire-turn'}}}));console.log(JSON.stringify({id:'callback',method:'item/commandExecution/requestApproval',params:{}}));}
else {assert.deepEqual(v,{id:'callback',error:{code:-32601,message:'unsupported callback'}});process.exit(0);}
});"#;
    let mut child = Command::new("node");
    child
        .args(["-e", script])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped());
    let tree = Tree::spawn(child).unwrap();
    let (tx, rx) = mpsc::channel();
    let mut proc = Process::connect(
        Arc::new(Mutex::new(tree)),
        EventSink::new("session".into(), tx),
        "session".into(),
        "thread".into(),
        Duration::from_secs(3),
        0,
    )
    .unwrap();
    proc.start_turn("turn", "test").unwrap();
    assert!(
        matches!(rx.recv_timeout(Duration::from_secs(3)).unwrap().1,HarnessEvent::Failed(reason) if reason=="unsupported Codex callback")
    );
    assert!(proc.tree.lock().unwrap().child().wait().unwrap().success());
}

#[test]
fn command_file_and_reroute_frames_keep_their_identity() {
    let mut wire = Wire::new("local-session".into(), "upstream".into());
    wire.begin("local-turn".into(), "remote-turn".into());
    let (tx, rx) = mpsc::channel();
    let sink = EventSink::new("local-session".into(), tx);
    let mut item = json!({"id":"c","type":"commandExecution","command":"echo test","aggregatedOutput":null,"status":"inProgress"});
    let params = |i: &Value| json!({"threadId":"upstream","turnId":"remote-turn","item":i});
    wire.notify("item/started", &params(&item), &sink).unwrap();
    wire.notify(
        "item/commandExecution/outputDelta",
        &json!({"threadId":"upstream","turnId":"remote-turn","itemId":"c","delta":"test"}),
        &sink,
    )
    .unwrap();
    item["status"] = json!("completed");
    item["exitCode"] = json!(0);
    wire.notify("item/completed", &params(&item), &sink)
        .unwrap();
    let changes = json!([{"path":"a","diff":"+one"},{"path":"b","diff":"-two"}]);
    wire.notify(
        "item/started",
        &params(&json!({"id":"f","type":"fileChange","changes":changes})),
        &sink,
    )
    .unwrap();
    wire.notify(
        "item/completed",
        &params(&json!({"id":"f","type":"fileChange","status":"completed","changes":changes})),
        &sink,
    )
    .unwrap();
    wire.notify(
        "model/rerouted",
        &json!({"threadId":"upstream","toModel":"rerouted"}),
        &sink,
    )
    .unwrap();
    let get = || rx.try_recv().unwrap().1;
    let HarnessEvent::Item(command) = get() else {
        panic!()
    };
    let id = json!(command)["itemId"].clone();
    let HarnessEvent::Delta { delta, .. } = get() else {
        panic!()
    };
    assert_eq!(delta.item_id, id);
    assert!(matches!(get(),HarnessEvent::CommandEnded{item,exit_code:Some(0),..} if item==id));
    let mut files = Vec::new();
    for _ in 0..4 {
        let HarnessEvent::Item(i) = get() else {
            panic!()
        };
        files.push(json!(i));
    }
    assert_ne!(files[0]["itemId"], files[1]["itemId"]);
    for n in 0..2 {
        assert_eq!(files[n]["itemId"], files[n + 2]["itemId"]);
        assert_eq!(files[n]["content"], changes[n]);
    }
    assert!(matches!(get(),HarnessEvent::Rerouted{model} if model=="rerouted"));
}
