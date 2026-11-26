//! Bootstrap of the local service. Task execution belongs to the adapter implementation.
mod auth;
mod client;
mod discovery;

pub use client::{DesktopClient, open_browser};
pub use discovery::Settings;

use auth::{BrowserSessions, secret};
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
use discovery::{Endpoint, private_dir, publish, runtime_dir};
use include_dir::{Dir, include_dir};
use qo_protocol::{InitializeResult, LoginLinkResult, ProtocolVersion, initialize_params};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    fs::OpenOptions,
    io::{self, IsTerminal},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tokio::net::TcpListener;

static WEB: Dir = include_dir!("$CARGO_MANIFEST_DIR/../../apps/web/dist");
const COOKIE: &str = "qo_session";

#[derive(Clone)]
struct Service {
    authority: String,
    origin: String,
    native: String,
    desktop: String,
    browser: Arc<Mutex<BrowserSessions>>,
}

#[derive(Clone, Copy, PartialEq)]
enum Client {
    Desktop,
    Browser,
    External,
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

    fn authenticate(&self, headers: &HeaderMap) -> Option<Client> {
        use subtle::ConstantTimeEq;
        if !self.same_origin(headers) {
            return None;
        }
        if headers.contains_key(header::AUTHORIZATION) {
            let token = one_header(headers, "authorization")?.strip_prefix("Bearer ")?;
            if token.as_bytes().ct_eq(self.desktop.as_bytes()).into() {
                return Some(Client::Desktop);
            }
            return bool::from(token.as_bytes().ct_eq(self.native.as_bytes()))
                .then_some(Client::External);
        }
        let mut cookies = one_header(headers, "cookie")?
            .split(';')
            .filter_map(|pair| {
                let (name, value) = pair.trim().split_once('=')?;
                (name == COOKIE).then_some(value)
            });
        let value = cookies.next()?;
        (cookies.next().is_none() && self.browser.lock().ok()?.accepts(value))
            .then_some(Client::Browser)
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

async fn assets(
    State(service): State<Service>,
    uri: axum::http::Uri,
    headers: HeaderMap,
) -> Response {
    if one_header(&headers, "host") != Some(&service.authority) {
        return StatusCode::FORBIDDEN.into_response();
    }
    let path = uri.path().trim_start_matches('/');
    let path = if path.is_empty() { "index.html" } else { path };
    let Some(file) = WEB.get_file(path) else {
        return StatusCode::NOT_FOUND.into_response();
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
    (headers, file.contents()).into_response()
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
    params: Option<Value>,
}

async fn rpc(mut socket: WebSocket, service: Service, client: Client) {
    let mut ready = false;
    while let Some(Ok(message)) = socket.recv().await {
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
        let result = match (ready, method.as_str()) {
            (false, "initialize") => match initialize_params(params.unwrap_or(Value::Null)) {
                Some(init) if init.versions.iter().any(|v| v == "qo.v1") => {
                    ready = true;
                    Ok(json!(InitializeResult {
                        version: ProtocolVersion::QoV1
                    }))
                }
                Some(_) => Err((-32001, "unsupported_version")),
                None => Err((-32602, "invalid_params")),
            },
            (false, _) => Err((-32002, "initialize_required")),
            (true, "ui.loginLink") if client == Client::Desktop => service
                .login_link()
                .map(|url| json!(LoginLinkResult { url }))
                .map_err(|_| (-32603, "internal_error")),
            (true, "ui.loginLink") => Err((-32003, "forbidden")),
            (true, _) => Err((-32601, "method_not_found")),
        };
        let response = match result {
            Ok(result) => json!({"jsonrpc":"2.0", "id":id, "result":result}),
            Err((code, reason)) => {
                json!({"jsonrpc":"2.0","id":id,"error":{"code":code,"message":reason,"data":{"reason":reason}}})
            }
        };
        if socket
            .send(Message::Text(response.to_string().into()))
            .await
            .is_err()
            || !ready
        {
            break;
        }
    }
}

pub fn run() -> io::Result<()> {
    tokio::runtime::Runtime::new()?.block_on(serve())
}

async fn serve() -> io::Result<()> {
    let settings = Settings::load()?;
    let dir = runtime_dir()?;
    private_dir(&dir)?;
    let instance = OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(dir.join("instance.lock"))?;
    instance
        .try_lock()
        .map_err(|e| io::Error::other(format!("QO instance lock: {e}")))?;
    let listener = TcpListener::bind(("127.0.0.1", settings.port))
        .await
        .map_err(|e| io::Error::new(e.kind(), format!("bind 127.0.0.1:{}: {e}", settings.port)))?;
    let endpoint = Endpoint {
        port: settings.port,
        token: secret()?,
    };
    let desktop = Endpoint {
        port: settings.port,
        token: secret()?,
    };
    let service = Service {
        authority: format!("127.0.0.1:{}", settings.port),
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
    if io::stdout().is_terminal() {
        println!("{first_link}");
    }
    let app = Router::new()
        .route("/auth/exchange", post(exchange))
        .route("/rpc", get(upgrade))
        .fallback(get(assets))
        .with_state(service);
    axum::serve(listener, app).await
}
