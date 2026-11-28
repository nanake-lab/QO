use std::io;

fn main() -> io::Result<()> {
    #[cfg(feature = "test-harness")]
    if std::env::args_os()
        .skip(1)
        .eq([std::ffi::OsString::from("--test-harness")])
    {
        return scripted();
    }
    if std::env::args_os().len() != 1 {
        return Err(io::Error::other("usage: qo-server"));
    }
    qo_server::run()
}

#[cfg(feature = "test-harness")]
fn scripted() -> std::io::Result<()> {
    use qo_harness::scripted::Scripted;
    let limit = std::env::var("QO_TEST_RETAIN_EVENTS")
        .ok()
        .map(|n| n.parse::<u64>())
        .transpose()
        .map_err(std::io::Error::other)?;
    let factory = Scripted {
        delivery_marker: std::env::var_os("QO_TEST_DELIVERY_MARKER").map(Into::into),
    };
    qo_server::run_service(Some(Box::new(factory)), |store| {
        store.limit_event_count(limit)
    })
}
