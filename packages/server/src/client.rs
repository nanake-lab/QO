use crate::discovery::{Endpoint, read_desktop_endpoint};
use qo_protocol::{InitializeResult, LoginLinkResult};
use serde_json::{Value, json};
use std::{
    io,
    net::{Ipv4Addr, SocketAddrV4, TcpStream},
    time::Duration,
};
use tungstenite::{Message, WebSocket, client::IntoClientRequest};

pub struct DesktopClient {
    endpoint: Endpoint,
}

impl DesktopClient {
    pub fn discover() -> io::Result<Self> {
        let client = Self {
            endpoint: read_desktop_endpoint()?,
        };
        client.connect()?;
        Ok(client)
    }

    fn connect(&self) -> io::Result<(WebSocket<TcpStream>, InitializeResult)> {
        // A blocked loopback endpoint must not hang desktop startup.
        let timeout = Duration::from_secs(2);
        let address = SocketAddrV4::new(Ipv4Addr::LOCALHOST, self.endpoint.port);
        let stream = TcpStream::connect_timeout(&address.into(), timeout)?;
        stream.set_read_timeout(Some(timeout))?;
        stream.set_write_timeout(Some(timeout))?;
        let mut upgrade = format!("ws://{address}/rpc")
            .into_client_request()
            .map_err(io::Error::other)?;
        upgrade.headers_mut().insert(
            "Origin",
            self.endpoint.origin().parse().map_err(io::Error::other)?,
        );
        upgrade.headers_mut().insert(
            "Authorization",
            format!("Bearer {}", self.endpoint.token)
                .parse()
                .map_err(io::Error::other)?,
        );
        // Errors returned to the renderer must not include the request or headers.
        let (mut ws, _) = tungstenite::client(upgrade, stream)
            .map_err(|_| io::Error::other("QO refused the desktop connection"))?;
        let result = call(&mut ws, 1, "initialize", json!({"versions":["qo.v1"]}))?;
        Ok((ws, serde_json::from_value(result)?))
    }

    pub fn initialize(&self) -> io::Result<InitializeResult> {
        self.connect().map(|(_, init)| init)
    }

    pub fn login_link(&self) -> io::Result<String> {
        let (mut ws, _) = self.connect()?;
        let result = call(&mut ws, 2, "ui.loginLink", json!({}))?;
        Ok(serde_json::from_value::<LoginLinkResult>(result)?.url)
    }
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
