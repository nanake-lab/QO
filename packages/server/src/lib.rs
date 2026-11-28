//! Shared authenticated loopback transport and the real service entry point.
mod auth;
mod backend;
mod client;
mod connection;
mod discovery;
pub use qo_harness as harness;
mod service;
pub use service::ServiceBackend;
pub mod storage;

pub use backend::{Backend, Failure, Notifications, Peer, Role, RpcResult};
pub use client::{DesktopClient, open_browser};
pub use connection::{Assets, serve_with, serve_with_factory};
pub use discovery::{Settings, private_dirs, runtime_dir};

#[cfg(feature = "embedded-web")]
pub fn run() -> std::io::Result<()> {
    run_service(None, |_| {})
}

#[cfg(feature = "embedded-web")]
pub fn run_service(
    factory: Option<Box<dyn harness::HarnessFactory>>,
    configure: impl FnOnce(&mut storage::Store),
) -> std::io::Result<()> {
    use std::io::IsTerminal;
    let settings = Settings::load()?;
    let dir = discovery::runtime_dir()?;
    tokio::runtime::Runtime::new()?.block_on(serve_with_factory(
        settings,
        dir,
        Assets::Embedded,
        |settings| {
            let mut store = storage::Store::open(settings).map_err(std::io::Error::other)?;
            configure(&mut store);
            Ok(ServiceBackend::new(
                store,
                settings,
                discovery::workspaces()?,
                factory,
            )?)
        },
        |_, link| {
            if std::io::stdout().is_terminal() {
                println!("{link}\nOpen a fresh link with qo open or qo open --print.");
            }
        },
    ))
}
