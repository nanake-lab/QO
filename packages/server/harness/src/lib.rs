//! Internal service/adapter interface for one assistant process.
#[cfg(feature = "test-harness")]
pub mod scripted;

use qo_protocol::{
    ApprovalKind, Choice, Id, InputAnswer, InputQuestion, Item, ItemDelta, RawRequest,
    SessionOpenParams, Task, TurnState, Usage,
};
use serde_json::{Map, Value};
use std::{collections::BTreeMap, io, sync::mpsc::Sender};

/// One independently owned process. Dropping a handle must release the process and its callbacks.
/// A successful reply means the response was written to that process.
pub trait Harness: Send {
    fn start_turn(&mut self, turn: &str, input: &str) -> io::Result<()>;
    fn append(&mut self, turn: &str, input: &str) -> io::Result<()>;
    fn interrupt(&mut self, turn: &str) -> io::Result<()>;
    /// Validate the submitted choice and form or permission fields against the frozen callback.
    fn valid_decision(
        &self,
        callback: &str,
        kind: ApprovalKind,
        choice: Choice,
        answer: Option<&Map<String, Value>>,
    ) -> bool;
    fn reply(&mut self, callback: &str, reply: Reply) -> io::Result<()>;
    fn finish(&mut self) -> io::Result<()>;
    fn terminate(&mut self);
}

pub trait HarnessFactory: Send + Sync {
    fn name(&self) -> &str;
    fn start(
        &self,
        task: &Task,
        params: &SessionOpenParams,
        events: EventSink,
    ) -> io::Result<Started>;
}

pub struct Started {
    pub process: Box<dyn Harness>,
    pub version: String,
    pub model: Option<String>,
    pub reasoning_effort: Option<String>,
    pub service_tier: Option<String>,
}

pub enum Reply {
    Decision {
        choice: Choice,
        answer: Option<Map<String, Value>>,
    },
    Input(BTreeMap<String, InputAnswer>),
}

pub struct Material {
    pub kind: ApprovalKind,
    pub raw: RawRequest,
    pub summary: String,
}

pub enum HarnessEvent {
    Item(Item),
    Delta {
        turn: Id,
        delta: ItemDelta,
    },
    Approval {
        callback: String,
        turn: Option<Id>,
        item: Option<Id>,
        material: Material,
    },
    Input {
        callback: String,
        turn: Id,
        questions: Vec<InputQuestion>,
    },
    Withdrawn {
        callback: String,
    },
    Usage(Usage),
    Rerouted {
        model: String,
    },
    TurnEnded {
        turn: Id,
        state: TurnState,
        error: Option<String>,
    },
    Exited,
}

/// Session identity is bound by the service, independently of any upstream payload.
#[derive(Clone)]
pub struct EventSink {
    session: Id,
    tx: Sender<(Id, HarnessEvent)>,
}
impl EventSink {
    pub fn new(session: Id, tx: Sender<(Id, HarnessEvent)>) -> Self {
        Self { session, tx }
    }

    pub fn send(&self, event: HarnessEvent) -> io::Result<()> {
        self.tx
            .send((self.session.clone(), event))
            .map_err(|_| io::Error::other("backend stopped"))
    }
}
