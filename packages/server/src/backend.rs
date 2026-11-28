use qo_protocol::{ErrorReason, Method, validate_output};
use serde_json::{Value, json};
use tokio::sync::{mpsc, watch};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    Desktop,
    Browser,
    External,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Peer {
    pub id: u64,
    pub role: Role,
}

#[derive(Debug, PartialEq)]
pub enum Failure {
    Reason(ErrorReason),
    CursorExpired { oldest: u64, high: u64 },
}

impl From<ErrorReason> for Failure {
    fn from(reason: ErrorReason) -> Self {
        Self::Reason(reason)
    }
}

impl Failure {
    pub(crate) fn wire(self) -> Value {
        let (reason, data) = match self {
            Self::Reason(reason) => (reason, json!({"reason":reason})),
            Self::CursorExpired { oldest, high } => (
                ErrorReason::CursorExpired,
                json!({"reason":"cursor_expired","oldestCursor":oldest.to_string(),"highWater":high.to_string()}),
            ),
        };
        json!({"code":reason.code(),"message":reason,"data":data})
    }
}

pub type RpcResult = Result<Value, Failure>;

pub(crate) enum Outbound {
    Frames(Vec<Value>),
    Replay(crate::storage::Replay),
}

/// Replay and live batches share one bounded FIFO per socket.
#[derive(Clone)]
pub struct Notifications {
    pub(crate) tx: mpsc::Sender<Outbound>,
    pub(crate) overflow: watch::Sender<bool>,
}
impl Notifications {
    pub fn push(&self, method: &str, params: &Value) {
        self.batch(method, std::slice::from_ref(params));
    }

    pub fn batch(&self, method: &str, values: &[Value]) {
        if values.is_empty() {
            return;
        }
        let frames = values
            .iter()
            .map(|params| {
                assert!(
                    validate_output(method, params),
                    "invalid notification: {method}"
                );
                json!({"jsonrpc":"2.0","method":method,"params":params})
            })
            .collect();
        self.enqueue(Outbound::Frames(frames));
    }

    pub(crate) fn enqueue(&self, message: Outbound) {
        if self.tx.try_send(message).is_err() {
            // Disconnect slow consumers; they can resume from their applied cursor.
            self.overflow.send_replace(true);
        }
    }
}

pub trait Backend: Send + Sync + 'static {
    fn connected(&self, _peer: Peer, _out: Notifications) {}
    fn disconnected(&self, _peer: Peer) {}
    fn call(&self, peer: Peer, method: Method, params: Value) -> RpcResult;
}
#[cfg(test)]
pub(crate) struct Bootstrap;
#[cfg(test)]
impl Backend for Bootstrap {
    fn call(&self, _: Peer, _: Method, _: Value) -> RpcResult {
        Err(ErrorReason::MethodNotFound.into())
    }
}
