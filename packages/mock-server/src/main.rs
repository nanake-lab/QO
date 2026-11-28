mod callbacks;
mod scenario;
mod state;
mod tasks;

use qo_protocol::Method;
use qo_server::{Assets, Peer, Role, Settings};
use serde_json::json;
use state::{Mock, State, id};
use std::{
    env, fs, io,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
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

fn seed(s: &mut State) {
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
    s.dispatch(
        peer,
        Method::TaskCreate,
        json!({"taskId":task, "name":"外部客户端示例","ownerKey":key}),
    )
    .unwrap();
    s.dispatch(
        peer,
        Method::SessionOpen,
        json!({"taskId":task,"sessionId":session,"model":"mock"}),
    )
    .unwrap();
    s.dispatch(
        peer,
        Method::TurnStart,
        json!({"sessionId":session,"turnId":id(),"input":"scenario:approval"}),
    )
    .unwrap();
    s.tick();
    s.owners.get_mut(&task).unwrap().holder = None;
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
    if let Ok(real) = qo_server::runtime_dir() {
        let real = resolved_path(&std::path::absolute(real)?)?;
        let run = resolved_path(&root.join("run"))?;
        if run.starts_with(&real)
            || real.starts_with(&run)
            || web.starts_with(&real)
            || real.starts_with(&web)
        {
            return Err(invalid("mock paths overlap the real runtime directory"));
        }
    }
    fs::create_dir_all(root.join("config"))?;
    let ttl = i64::try_from(o.ttl)
        .ok()
        .and_then(chrono::TimeDelta::try_seconds)
        .filter(|ttl| chrono::Utc::now().checked_add_signed(*ttl).is_some())
        .ok_or_else(|| invalid("invalid approval TTL"))?;
    let mut state = State::new(o.retain, ttl, root.join("workspaces"));
    if o.seed {
        seed(&mut state);
    }
    let backend = Arc::new(Mock(Mutex::new(state)));
    let playback = backend.clone();
    tokio::spawn(async move {
        // One scheduler serializes script steps with RPC changes; 25 ms bounds UI playback jitter.
        let mut interval = tokio::time::interval(Duration::from_millis(25));
        loop {
            interval.tick().await;
            playback.0.lock().unwrap().tick();
        }
    });
    qo_server::serve_with(
        Settings {
            port: o.port,
            login_link_seconds: o.login_ttl,
            ..Settings::default()
        },
        root.join("run"),
        Assets::Directory(web),
        backend,
        |origin, link| {
            println!("{origin}\n{}\n{link}", root.display());
        },
    )
    .await
}
