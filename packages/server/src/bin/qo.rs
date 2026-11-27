use std::{env, io};

fn main() -> io::Result<()> {
    let mut args = env::args_os().skip(1);
    if args.next().as_deref() == Some("open".as_ref()) {
        match (args.next(), args.next()) {
            (None, None) => return qo_server::open_browser(),
            (Some(flag), None) if flag == "--print" => {
                println!("{}", qo_server::DesktopClient::discover()?.login_link()?);
                return Ok(());
            }
            _ => {}
        }
    }
    Err(io::Error::other("usage: qo open [--print]"))
}
