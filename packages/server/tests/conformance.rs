use qo_conformance::Real;
const TARGET: Real = Real(env!("CARGO_BIN_EXE_qo-server"));
mod connection {
    use super::TARGET;
    #[test]
    fn headers_and_credentials() {
        qo_conformance::connection::headers_and_credentials(&TARGET);
    }
    #[test]
    fn negotiation_and_roles() {
        qo_conformance::connection::negotiation_and_roles(&TARGET);
    }
    #[test]
    fn restart_and_single_use() {
        qo_conformance::connection::restart_and_single_use(&TARGET);
    }
    #[test]
    fn link_expiry() {
        qo_conformance::connection::link_expiry(&TARGET);
    }
}
