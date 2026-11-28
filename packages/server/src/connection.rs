use crate::auth::{BrowserSessions, secret};
use crate::discovery::{Endpoint, private_dir, publish};
use crate::{Backend, Notifications, Peer, Role, RpcResult, Settings};
use axum::{
    Json, Router,
    extract::{
        State, WebSocketUpgrade,
        ws::{Message, WebSocket},
    },
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use qo_protocol::{
    ErrorReason, InitializeParams, InitializeResult, LoginLinkResult, Method, ProtocolVersion,
    validate_params,
};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    fs::OpenOptions,
    io,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use std::{
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};
use tokio::net::TcpListener;
use tokio::sync::{mpsc, watch};

#[cfg(feature = "embedded-web")]
static WEB: include_dir::Dir = include_dir::include_dir!("$CARGO_MANIFEST_DIR/../../apps/web/dist");
const COOKIE: &str = "qo_session";

#[derive(Clone)]
struct Service {
    authority: String,
    origin: String,
    assets: Assets,
    backend: Arc<dyn Backend>,
    next_peer: Arc<AtomicU64>,
    native: String,
    desktop: String,
    browser: Arc<Mutex<BrowserSessions>>,
}

#[derive(Clone)]
pub enum Assets {
    #[cfg(feature = "embedded-web")]
    Embedded,
    Directory(PathBuf),
}

fn one_header<'a>(headers: &'a HeaderMap, key: &str) -> Option<&'a str> {
    let mut values = headers.get_all(key).iter();
    let value = values.next()?.to_str().ok()?;
    values.next().is_none().then_some(value)
}

impl Service {
    fn same_origin(&self, headers: &HeaderMap) -> bool {
        one_header(headers, "host") == Some(&self.authority)
            && one_header(headers, "origin") == Some(&self.origin)
    }

    fn authenticate(&self, headers: &HeaderMap) -> Option<Role> {
        use subtle::ConstantTimeEq;
        if !self.same_origin(headers) {
            return None;
        }
        if headers.contains_key(header::AUTHORIZATION) {
            let token = one_header(headers, "authorization")?.strip_prefix("Bearer ")?;
            if token.as_bytes().ct_eq(self.desktop.as_bytes()).into() {
                return Some(Role::Desktop);
            }
            return bool::from(token.as_bytes().ct_eq(self.native.as_bytes()))
                .then_some(Role::External);
        }
        let mut cookies = one_header(headers, "cookie")?
            .split(';')
            .filter_map(|pair| {
                let (name, value) = pair.trim().split_once('=')?;
                (name == COOKIE).then_some(value)
            });
        let value = cookies.next()?;
        (cookies.next().is_none() && self.browser.lock().ok()?.accepts(value))
            .then_some(Role::Browser)
    }

    fn login_link(&self) -> io::Result<String> {
        let code = self
            .browser
            .lock()
            .map_err(|_| io::Error::other("login state unavailable"))?
            .issue(Instant::now())?;
        Ok(format!("{}/#login={code}", self.origin))
    }
}

#[derive(Deserialize)]
struct Exchange {
    code: String,
}
async fn exchange(
    State(service): State<Service>,
    headers: HeaderMap,
    Json(body): Json<Exchange>,
) -> Response {
    if !service.same_origin(&headers) {
        return StatusCode::FORBIDDEN.into_response();
    }
    let Ok(mut browser) = service.browser.lock() else {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };
    match browser.exchange(&body.code, Instant::now()) {
        Ok(Some(cookie)) => (
            StatusCode::NO_CONTENT,
            [
                (
                    header::SET_COOKIE,
                    format!("{COOKIE}={cookie}; Path=/rpc; HttpOnly; SameSite=Strict"),
                ),
                (header::CACHE_CONTROL, "no-store".into()),
            ],
        )
            .into_response(),
        Ok(None) => StatusCode::FORBIDDEN.into_response(),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

async fn static_file(
    State(service): State<Service>,
    uri: axum::http::Uri,
    headers: HeaderMap,
) -> Response {
    if one_header(&headers, "host") != Some(&service.authority) {
        return StatusCode::FORBIDDEN.into_response();
    }
    let path = uri.path().trim_start_matches('/');
    let path = if path.is_empty() { "index.html" } else { path };
    let body: axum::body::Body = match &service.assets {
        #[cfg(feature = "embedded-web")]
        Assets::Embedded => match WEB.get_file(path) {
            Some(file) => file.contents().into(),
            None => return StatusCode::NOT_FOUND.into_response(),
        },
        Assets::Directory(root) => {
            if path.contains(['\\', ':'])
                || Path::new(path)
                    .components()
                    .any(|c| !matches!(c, std::path::Component::Normal(_)))
            {
                return StatusCode::NOT_FOUND.into_response();
            }
            let Ok(file) = tokio::fs::canonicalize(root.join(path)).await else {
                return StatusCode::NOT_FOUND.into_response();
            };
            if !file.starts_with(root) {
                return StatusCode::NOT_FOUND.into_response();
            }
            match tokio::fs::read(file).await {
                Ok(bytes) => bytes.into(),
                Err(_) => return StatusCode::NOT_FOUND.into_response(),
            }
        }
    };
    let mime = match path.rsplit('.').next() {
        Some("html") => "text/html; charset=utf-8",
        Some("js") => "text/javascript; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        _ => "application/octet-stream",
    };
    let policy = format!(
        "default-src 'self'; connect-src 'self' ws://{}; frame-ancestors 'none'; base-uri 'none'",
        service.authority,
    );
    let headers = [
        (header::CONTENT_TYPE, mime),
        (header::REFERRER_POLICY, "no-referrer"),
        (header::X_CONTENT_TYPE_OPTIONS, "nosniff"),
        (header::CONTENT_SECURITY_POLICY, policy.as_str()),
    ];
    (headers, body).into_response()
}

async fn upgrade(
    State(service): State<Service>,
    headers: HeaderMap,
    ws: WebSocketUpgrade,
) -> Response {
    match service.authenticate(&headers) {
        Some(client) => ws
            .on_upgrade(move |socket| rpc(socket, service, client))
            .into_response(),
        None => StatusCode::FORBIDDEN.into_response(),
    }
}

#[derive(Deserialize)]
struct Request {
    jsonrpc: String,
    id: Value,
    method: String,
    #[serde(default = "empty_params")]
    params: Value,
}

fn empty_params() -> Value {
    json!({})
}

async fn dispatch(
    ready: &mut bool,
    method_name: &str,
    params: Value,
    peer: Peer,
    service: &Service,
) -> RpcResult {
    let method = Method::from_name(method_name);
    if !*ready && method != Some(Method::Initialize) {
        return Err(ErrorReason::InitializeRequired.into());
    }
    let method = method.ok_or(ErrorReason::MethodNotFound)?;
    if !validate_params(method.name(), &params) {
        return Err(ErrorReason::InvalidParams.into());
    }
    match method {
        Method::Initialize if !*ready => {
            let init: InitializeParams =
                serde_json::from_value(params).map_err(|_| ErrorReason::InvalidParams)?;
            if !init.versions.iter().any(|v| v == "qo.v1") {
                return Err(ErrorReason::UnsupportedVersion.into());
            }
            *ready = true;
            Ok(json!(InitializeResult {
                version: ProtocolVersion::QoV1
            }))
        }
        Method::UiLoginLink if peer.role == Role::Desktop => service
            .login_link()
            .map(|url| json!(LoginLinkResult { url }))
            .map_err(|_| ErrorReason::InternalError.into()),
        Method::UiLoginLink => Err(ErrorReason::Forbidden.into()),
        Method::Initialize => Err(ErrorReason::MethodNotFound.into()),
        _ if peer.role == Role::Desktop => Err(ErrorReason::Forbidden.into()),
        business => {
            let backend = service.backend.clone();
            tokio::task::spawn_blocking(move || backend.call(peer, business, params))
                .await
                .map_err(|_| ErrorReason::InternalError)?
        }
    }
}

async fn rpc(mut socket: WebSocket, service: Service, client: Role) {
    let peer = Peer {
        id: service.next_peer.fetch_add(1, Ordering::Relaxed),
        role: client,
    };
    // Bound queued live batches per socket so an idle browser cannot hold the service back.
    let (tx, mut rx) = mpsc::channel(256);
    let (overflow, mut lagged) = watch::channel(false);
    let out = Notifications { tx, overflow };
    let mut ready = false;
    'socket: loop {
        let message = tokio::select! {
            _ = lagged.changed() => break,
            Some(outbound) = rx.recv() => {
                match outbound {
                    crate::backend::Outbound::Replay(mut replay) => loop {
                        let Ok(events) = replay.next_page() else {break 'socket};
                        if events.is_empty() { break; }
                        for event in events {
                            let frame = json!({"jsonrpc":"2.0","method":"event","params":event});
                            if !send(&mut socket, frame, &mut lagged).await {break 'socket;}
                        }
                    },
                    crate::backend::Outbound::Frames(frames) => for frame in frames {
                        if !send(&mut socket, frame, &mut lagged).await {break 'socket;}
                    }
                }
                continue;
            }
            incoming = socket.recv() => match incoming {Some(Ok(frame))=>frame,_=>break}
        };
        let text = match message {
            Message::Text(text) => text,
            Message::Ping(_) | Message::Pong(_) => continue,
            _ => break,
        };
        let Ok(Request {
            jsonrpc,
            id,
            method,
            params,
        }) = serde_json::from_str(&text)
        else {
            break;
        };
        if jsonrpc != "2.0" || !(id.is_string() || id.is_number()) {
            break;
        }
        let was_ready = ready;
        let result = dispatch(&mut ready, &method, params, peer, &service).await;
        if ready && !was_ready {
            service.backend.connected(peer, out.clone());
        }
        let response = match result {
            Ok(result) => {
                assert!(
                    qo_protocol::validate_output(&method, &result),
                    "invalid result: {method}"
                );
                json!({"jsonrpc":"2.0","id":id,"result":result})
            }
            Err(failure) => {
                let error = failure.wire();
                assert!(
                    qo_protocol::validate_error(&error),
                    "invalid error: {method}"
                );
                json!({"jsonrpc":"2.0","id":id,"error":error})
            }
        };
        if !send(&mut socket, response, &mut lagged).await || !ready {
            break;
        }
    }
    if ready {
        service.backend.disconnected(peer);
    }
}

async fn send(socket: &mut WebSocket, value: Value, lagged: &mut watch::Receiver<bool>) -> bool {
    tokio::select! {
        biased;
        _ = lagged.changed() => false,
        sent = socket.send(Message::Text(value.to_string().into())) => sent.is_ok(),
    }
}

pub(crate) fn lock_instance(dir: &Path) -> io::Result<std::fs::File> {
    private_dir(dir)?;
    let instance = OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(dir.join("instance.lock"))?;
    instance
        .try_lock()
        .map_err(|e| io::Error::other(format!("QO instance lock: {e}")))?;
    Ok(instance)
}

pub async fn serve_with(
    settings: Settings,
    dir: PathBuf,
    assets: Assets,
    backend: Arc<dyn Backend>,
    started: impl FnOnce(&str, &str),
) -> io::Result<()> {
    let _instance = lock_instance(&dir)?;
    serve_locked(settings, dir, assets, backend, started).await
}

pub async fn serve_with_factory(
    settings: Settings,
    dir: PathBuf,
    assets: Assets,
    build: impl FnOnce(&Settings) -> io::Result<Arc<dyn Backend>>,
    started: impl FnOnce(&str, &str),
) -> io::Result<()> {
    let _instance = lock_instance(&dir)?;
    let backend = build(&settings)?;
    serve_locked(settings, dir, assets, backend, started).await
}

pub(crate) async fn serve_locked(
    settings: Settings,
    dir: PathBuf,
    assets: Assets,
    backend: Arc<dyn Backend>,
    started: impl FnOnce(&str, &str),
) -> io::Result<()> {
    let listener = TcpListener::bind(("127.0.0.1", settings.port))
        .await
        .map_err(|e| io::Error::new(e.kind(), format!("bind 127.0.0.1:{}: {e}", settings.port)))?;
    let port = listener.local_addr()?.port();
    let endpoint = Endpoint {
        port,
        token: secret()?,
    };
    let desktop = Endpoint {
        port,
        token: secret()?,
    };
    backend.remember_secret(&endpoint.token);
    backend.remember_secret(&desktop.token);
    let service = Service {
        authority: format!("127.0.0.1:{port}"),
        assets,
        backend,
        next_peer: Arc::new(AtomicU64::new(1)),
        origin: endpoint.origin(),
        native: endpoint.token.clone(),
        desktop: desktop.token.clone(),
        browser: Arc::new(Mutex::new(BrowserSessions::new(Duration::from_secs(
            settings.login_link_seconds,
        )))),
    };
    publish(&dir, "endpoint.json", &endpoint)?;
    publish(&dir, "desktop.json", &desktop)?;
    let first_link = service.login_link()?;
    started(&service.origin, &first_link);
    let app = Router::new()
        .route("/auth/exchange", post(exchange))
        .route("/rpc", get(upgrade))
        .fallback(get(static_file))
        .with_state(service);
    axum::serve(listener, app).await
}

#[cfg(test)]
#[path = "connection_tests.rs"]
mod tests;
