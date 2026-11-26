use std::{env, io};

fn main() -> io::Result<()> {
    if env::args_os()
        .skip(1)
        .eq([std::ffi::OsString::from("open")])
    {
        qo_server::open_browser()
    } else {
        Err(io::Error::other("usage: qo open"))
    }
}
