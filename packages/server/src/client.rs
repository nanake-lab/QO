use crate::discovery::read_desktop_endpoint;
use qo_protocol::{InitializeResult, LoginLinkResult};
use serde_json::{Value, json};
use std::{
    io,
    net::{Ipv4Addr, SocketAddrV4, TcpStream},
    time::Duration,
};
use tungstenite::{Message, WebSocket, client::IntoClientRequest};

pub struct DesktopClient {
    socket: WebSocket<TcpStream>,
    origin: String,
}

impl DesktopClient {
    pub fn discover() -> io::Result<Self> {
        let endpoint = read_desktop_endpoint()?;
        // Bound each operation on the local service while opening the UI.
        let timeout = Duration::from_secs(2);
        let address = SocketAddrV4::new(Ipv4Addr::LOCALHOST, endpoint.port);
        let stream = TcpStream::connect_timeout(&address.into(), timeout)?;
        stream.set_read_timeout(Some(timeout))?;
        stream.set_write_timeout(Some(timeout))?;
        let mut upgrade = format!("ws://{address}/rpc")
            .into_client_request()
            .map_err(io::Error::other)?;
        upgrade.headers_mut().insert(
            "Origin",
            endpoint.origin().parse().map_err(io::Error::other)?,
        );
        upgrade.headers_mut().insert(
            "Authorization",
            format!("Bearer {}", endpoint.token)
                .parse()
                .map_err(io::Error::other)?,
        );
        // Handshake errors may include credential headers.
        let (mut ws, _) = tungstenite::client(upgrade, stream)
            .map_err(|_| io::Error::other("QO refused the desktop connection"))?;
        let result = call(&mut ws, 1, "initialize", json!({"versions":["qo.v1"]}))?;
        let _: InitializeResult = serde_json::from_value(result)?;
        Ok(Self {
            socket: ws,
            origin: endpoint.origin(),
        })
    }

    pub fn login_link(mut self) -> io::Result<String> {
        let result = call(&mut self.socket, 2, "ui.loginLink", json!({}))?;
        let link = serde_json::from_value::<LoginLinkResult>(result)?.url;
        let prefix = format!("{}/#login=", self.origin);
        if !link
            .strip_prefix(&prefix)
            .is_some_and(|code| !code.is_empty() && code.bytes().all(|c| c.is_ascii_hexdigit()))
        {
            return Err(io::Error::other("invalid login URL from QO"));
        }
        Ok(link)
    }
}

/// Open a newly issued login link with the OS default browser.
pub fn open_browser() -> io::Result<()> {
    let client = DesktopClient::discover().map_err(|e| {
        io::Error::new(
            e.kind(),
            format!("could not connect to the local QO service: {e}"),
        )
    })?;
    open::that_detached(client.login_link()?)
        .map_err(|_| io::Error::other("could not open the default browser"))
}

fn call(ws: &mut WebSocket<TcpStream>, id: u8, method: &str, params: Value) -> io::Result<Value> {
    let request = json!({"jsonrpc":"2.0","id":id,"method":method,"params":params});
    ws.send(Message::Text(request.to_string().into()))
        .map_err(io::Error::other)?;
    let message = ws.read().map_err(io::Error::other)?;
    let result: Value = serde_json::from_str(message.to_text().map_err(io::Error::other)?)?;
    if result["id"] != id || result["jsonrpc"] != "2.0" || result.get("result").is_none() {
        return Err(io::Error::other("QO bootstrap request failed"));
    }
    Ok(result["result"].clone())
}
