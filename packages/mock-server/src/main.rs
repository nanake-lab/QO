use qo_harness::scripted::Scripted;
use qo_protocol::Method;
use qo_server::{Assets, Backend, Peer, Role, ServiceBackend, Settings, storage::Store};
use serde_json::json;
fn id() -> String {
    uuid::Uuid::new_v4().to_string()
}
use std::{
    env, fs, io,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};
use tempfile::TempDir;

struct Options {
    port: u16,
    root: Option<PathBuf>,
    web: PathBuf,
    ttl: u64,
    login_ttl: u64,
    retain: Option<usize>,
    seed: bool,
}
fn invalid(msg: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, msg)
}
impl Options {
    fn read() -> io::Result<Self> {
        // Adjacent to real QO, while allowing both listeners to run.
        let mut o = Self {
            port: 47262,
            root: None,
            web: "apps/web/dist".into(),
            ttl: 600,
            login_ttl: Settings::default().login_link_seconds,
            retain: None,
            seed: false,
        };
        let mut args = env::args_os().skip(1);
        while let Some(flag) = args.next() {
            let val = args.next().ok_or_else(|| invalid("missing option value"))?;
            match flag.to_str() {
                Some("--root") => o.root = Some(val.into()),
                Some("--web") => o.web = val.into(),
                Some("--port") => {
                    o.port = val
                        .to_str()
                        .and_then(|s| s.parse().ok())
                        .ok_or_else(|| invalid("invalid port"))?
                }
                Some("--login-link-seconds") => {
                    o.login_ttl = val
                        .to_str()
                        .and_then(|s| s.parse::<u64>().ok())
                        .filter(|n| {
                            *n > 0
                                && Instant::now()
                                    .checked_add(Duration::from_secs(*n))
                                    .is_some()
                        })
                        .ok_or_else(|| invalid("invalid login link TTL"))?;
                }
                Some("--approval-ttl-seconds") => {
                    o.ttl = val
                        .to_str()
                        .and_then(|s| s.parse().ok())
                        .ok_or_else(|| invalid("invalid approval TTL"))?
                }
                Some("--retain-events") => {
                    o.retain = Some(
                        val.to_str()
                            .and_then(|s| s.parse().ok())
                            .ok_or_else(|| invalid("invalid event retention"))?,
                    )
                }
                Some("--seed") if val == "external" => o.seed = true,
                _ => {
                    return Err(invalid(
                        "usage: qo-mock [--port N] [--root DIR] [--web DIR] [--login-link-seconds N] [--approval-ttl-seconds N] [--retain-events N] [--seed external]",
                    ));
                }
            }
        }
        Ok(o)
    }
}

fn seed(s: &ServiceBackend) -> io::Result<()> {
    // No real client has the synthetic creator's private key or connection.
    let peer = Peer {
        id: 0,
        role: Role::External,
    };
    let task = id();
    let session = id();
    let key = format!(
        "{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    );
    s.call(
        peer,
        Method::TaskCreate,
        json!({"taskId":task, "name":"外部客户端示例","ownerKey":key}),
    )
    .unwrap();
    s.call(
        peer,
        Method::SessionOpen,
        json!({"taskId":task,"sessionId":session,"model":"mock"}),
    )
    .unwrap();
    s.call(
        peer,
        Method::TurnStart,
        json!({"sessionId":session,"turnId":id(),"input":"scenario:approval"}),
    )
    .unwrap();
    // The three startup lines promise a ready example, including its pending approval.
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let list = s
            .call(peer, Method::ApprovalList, json!({"taskId":task}))
            .map_err(|_| io::Error::other("seed task unavailable"))?;
        if list["requests"]
            .as_array()
            .is_some_and(|requests| !requests.is_empty())
        {
            break;
        }
        if Instant::now() >= deadline {
            return Err(io::Error::other("seed scenario did not request approval"));
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    s.disconnected(peer);
    Ok(())
}

// The web build may start after the mock. Resolve its existing ancestor, including symlinks.
fn resolved_path(path: &Path) -> io::Result<PathBuf> {
    match fs::canonicalize(path) {
        Ok(p) => Ok(p),
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            let parent = path.parent().ok_or(e)?;
            Ok(resolved_path(parent)?.join(
                path.file_name()
                    .ok_or_else(|| invalid("invalid web path"))?,
            ))
        }
        Err(e) => Err(e),
    }
}

#[tokio::main]
async fn main() -> io::Result<()> {
    let o = Options::read()?;
    let temp = if o.root.is_none() {
        Some(TempDir::new()?)
    } else {
        None
    };
    let root = o
        .root
        .as_deref()
        .unwrap_or_else(|| temp.as_ref().unwrap().path());
    fs::create_dir_all(root)?;
    let root = fs::canonicalize(root)?;
    let web = resolved_path(&std::path::absolute(o.web)?)?;
    if root.starts_with(&web)
        || web.starts_with(root.join("run"))
        || web.starts_with(root.join("config"))
    {
        return Err(invalid("web assets overlap private mock files"));
    }
    if let Ok(private_dirs) = qo_server::private_dirs() {
        let run = resolved_path(&root.join("run"))?;
        for path in private_dirs {
            let real = resolved_path(&std::path::absolute(path)?)?;
            if run.starts_with(&real)
                || real.starts_with(&run)
                || web.starts_with(&real)
                || real.starts_with(&web)
            {
                return Err(invalid("mock paths overlap private service files"));
            }
        }
    }
    fs::create_dir_all(root.join("config"))?;
    let settings = Settings {
        port: o.port,
        login_link_seconds: o.login_ttl,
        approval_ttl_seconds: o.ttl,
        ..Settings::default()
    };
    let state = tempfile::Builder::new()
        .prefix("qo-state-")
        .tempdir_in(&root)?;
    let state_path = state.path().canonicalize()?;
    if state_path.starts_with(&web) || web.starts_with(&state_path) {
        return Err(invalid("web assets overlap private mock state"));
    }
    qo_server::serve_with_factory(
        settings,
        root.join("run"),
        Assets::Directory(web),
        |settings| {
            let mut store = Store::open_in(&state_path, settings).map_err(io::Error::other)?;
            store.limit_event_count(o.retain.map(|n| n as u64));
            let backend = ServiceBackend::new(
                store,
                settings,
                root.join("workspaces"),
                Some(Box::new(Scripted::default())),
            )?;
            if o.seed {
                seed(&backend)?;
            }
            Ok(backend)
        },
        |origin, link| println!("{origin}\n{}\n{link}", root.display()),
    )
    .await
}
