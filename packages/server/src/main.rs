use std::io;

fn main() -> io::Result<()> {
    if std::env::args_os().len() != 1 {
        return Err(io::Error::other("usage: qo-server"));
    }
    qo_server::run()
}
