use std::io;

fn main() -> io::Result<()> {
    match std::env::args().nth(1).as_deref() {
        None => qo_server::run(),
        Some("login-link") => {
            println!("{}", qo_server::DesktopClient::discover()?.login_link()?);
            Ok(())
        }
        Some(_) => Err(io::Error::other("usage: qo-server [login-link]")),
    }
}
