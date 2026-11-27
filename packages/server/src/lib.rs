//! Shared authenticated loopback transport and the real service entry point.
mod auth;
mod backend;
mod client;
mod connection;
mod discovery;

pub use backend::{Backend, Failure, Notifications, Peer, Role, RpcResult};
pub use client::{DesktopClient, open_browser};
pub use connection::{Assets, serve_with};
pub use discovery::{Settings, runtime_dir};

#[cfg(feature = "embedded-web")]
pub fn run() -> std::io::Result<()> {
    use std::{io::IsTerminal, sync::Arc};
    tokio::runtime::Runtime::new()?.block_on(serve_with(
        Settings::load()?,
        discovery::runtime_dir()?,
        Assets::Embedded,
        Arc::new(backend::Bootstrap),
        |_, link| {
            if std::io::stdout().is_terminal() {
                println!("{link}\nOpen a fresh link with qo open or qo open --print.");
            }
        },
    ))
}
