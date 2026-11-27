// Generated from the QO protocol schema. Run pnpm generate to update.

pub type Id = String;

pub type Timestamp = String;

pub type Decimal = String;

pub type Digest = String;

pub type OwnerKey = String;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ProtocolVersion {
    #[serde(rename = "qo.v1")]
    QoV1,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Creator {
    #[serde(rename = "ui")]
    Ui,
    #[serde(rename = "external")]
    External,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum CancelledBy {
    #[serde(rename = "owner")]
    Owner,
    #[serde(rename = "ui")]
    Ui,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum TaskState {
    #[serde(rename = "open")]
    Open,
    #[serde(rename = "closed")]
    Closed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum SessionState {
    #[serde(rename = "starting")]
    Starting,
    #[serde(rename = "idle")]
    Idle,
    #[serde(rename = "running")]
    Running,
    #[serde(rename = "waiting_input")]
    WaitingInput,
    #[serde(rename = "waiting_approval")]
    WaitingApproval,
    #[serde(rename = "completed")]
    Completed,
    #[serde(rename = "failed")]
    Failed,
    #[serde(rename = "cancelled")]
    Cancelled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum TurnState {
    #[serde(rename = "running")]
    Running,
    #[serde(rename = "completed")]
    Completed,
    #[serde(rename = "interrupted")]
    Interrupted,
    #[serde(rename = "failed")]
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ItemState {
    #[serde(rename = "running")]
    Running,
    #[serde(rename = "completed")]
    Completed,
    #[serde(rename = "failed")]
    Failed,
    #[serde(rename = "interrupted")]
    Interrupted,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ApprovalState {
    #[serde(rename = "pending")]
    Pending,
    #[serde(rename = "decided")]
    Decided,
    #[serde(rename = "expired")]
    Expired,
    #[serde(rename = "withdrawn")]
    Withdrawn,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ApprovalKind {
    #[serde(rename = "command_execution")]
    CommandExecution,
    #[serde(rename = "file_change")]
    FileChange,
    #[serde(rename = "permissions")]
    Permissions,
    #[serde(rename = "elicitation")]
    Elicitation,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Choice {
    #[serde(rename = "allow_once")]
    AllowOnce,
    #[serde(rename = "deny")]
    Deny,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Delivery {
    #[serde(rename = "recorded")]
    Recorded,
    #[serde(rename = "sent")]
    Sent,
    #[serde(rename = "unknown")]
    Unknown,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Task {
    pub task_id: Id,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub group: Option<String>,
    pub cwd: String,
    pub forbidden_roots: Vec<String>,
    pub created_by: Creator,
    pub state: TaskState,
    pub created_at: Timestamp,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub closed_at: Option<Timestamp>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Session {
    pub session_id: Id,
    pub task_id: Id,
    pub harness: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning_effort: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub service_tier: Option<String>,
    pub state: SessionState,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    pub created_at: Timestamp,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cancelled_by: Option<CancelledBy>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ended_at: Option<Timestamp>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Turn {
    pub turn_id: Id,
    pub session_id: Id,
    pub input: String,
    pub state: TurnState,
    pub started_at: Timestamp,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ended_at: Option<Timestamp>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CommandContent {
    pub command: String,
    pub output: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i64>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FileContent {
    pub path: String,
    pub diff: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", rename_all_fields = "camelCase", deny_unknown_fields)]
pub enum Item {
    #[serde(rename = "assistant_message")]
    AssistantMessage {
        item_id: Id,
        turn_id: Id,
        state: ItemState,
        started_at: Timestamp,
        #[serde(skip_serializing_if = "Option::is_none")]
        ended_at: Option<Timestamp>,
        content: String,
    },
    #[serde(rename = "command_execution")]
    CommandExecution {
        item_id: Id,
        turn_id: Id,
        state: ItemState,
        started_at: Timestamp,
        #[serde(skip_serializing_if = "Option::is_none")]
        ended_at: Option<Timestamp>,
        content: CommandContent,
    },
    #[serde(rename = "file_change")]
    FileChange {
        item_id: Id,
        turn_id: Id,
        state: ItemState,
        started_at: Timestamp,
        #[serde(skip_serializing_if = "Option::is_none")]
        ended_at: Option<Timestamp>,
        content: FileContent,
    },
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RawRequest {
    pub method: String,
    pub params: serde_json::Map<String, serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context: Option<serde_json::Map<String, serde_json::Value>>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ApprovalRequest {
    pub approval_id: Id,
    pub task_id: Id,
    pub session_id: Id,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub turn_id: Option<Id>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub item_id: Option<Id>,
    pub kind: ApprovalKind,
    pub raw: RawRequest,
    pub summary: String,
    pub digest: Digest,
    pub expires_at: Timestamp,
    pub state: ApprovalState,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ApprovalDecision {
    pub decision_id: Id,
    pub approval_id: Id,
    pub digest: Digest,
    pub choice: Choice,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub answer: Option<serde_json::Map<String, serde_json::Value>>,
    pub decided_at: Timestamp,
    pub delivery: Delivery,
}

pub type UsageCount = Option<String>;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UsageCounts {
    pub total_tokens: UsageCount,
    pub input_tokens: UsageCount,
    pub cached_input_tokens: UsageCount,
    pub output_tokens: UsageCount,
    pub reasoning_output_tokens: UsageCount,
    pub cache_write_input_tokens: UsageCount,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Usage {
    pub session_id: Id,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub turn_id: Option<Id>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning_effort: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub service_tier: Option<String>,
    pub source: String,
    pub counts: UsageCounts,
    pub observed_at: Timestamp,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ItemDelta {
    pub item_id: Id,
    pub delta: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", rename_all_fields = "camelCase", deny_unknown_fields)]
pub enum Event {
    #[serde(rename = "task.changed")]
    TaskChanged {
        cursor: String,
        at: Timestamp,
        task_id: Id,
        #[serde(skip_serializing_if = "Option::is_none")]
        session_id: Option<Id>,
        #[serde(skip_serializing_if = "Option::is_none")]
        turn_id: Option<Id>,
        payload: Task,
    },
    #[serde(rename = "session.changed")]
    SessionChanged {
        cursor: String,
        at: Timestamp,
        task_id: Id,
        #[serde(skip_serializing_if = "Option::is_none")]
        session_id: Option<Id>,
        #[serde(skip_serializing_if = "Option::is_none")]
        turn_id: Option<Id>,
        payload: Session,
    },
    #[serde(rename = "turn.changed")]
    TurnChanged {
        cursor: String,
        at: Timestamp,
        task_id: Id,
        #[serde(skip_serializing_if = "Option::is_none")]
        session_id: Option<Id>,
        #[serde(skip_serializing_if = "Option::is_none")]
        turn_id: Option<Id>,
        payload: Turn,
    },
    #[serde(rename = "item.changed")]
    ItemChanged {
        cursor: String,
        at: Timestamp,
        task_id: Id,
        #[serde(skip_serializing_if = "Option::is_none")]
        session_id: Option<Id>,
        #[serde(skip_serializing_if = "Option::is_none")]
        turn_id: Option<Id>,
        payload: Item,
    },
    #[serde(rename = "item.delta")]
    ItemDelta {
        cursor: String,
        at: Timestamp,
        task_id: Id,
        #[serde(skip_serializing_if = "Option::is_none")]
        session_id: Option<Id>,
        #[serde(skip_serializing_if = "Option::is_none")]
        turn_id: Option<Id>,
        payload: ItemDelta,
    },
    #[serde(rename = "usage.updated")]
    UsageUpdated {
        cursor: String,
        at: Timestamp,
        task_id: Id,
        #[serde(skip_serializing_if = "Option::is_none")]
        session_id: Option<Id>,
        #[serde(skip_serializing_if = "Option::is_none")]
        turn_id: Option<Id>,
        payload: Usage,
    },
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct InitializeParams {
    pub versions: Vec<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct InitializeResult {
    pub version: ProtocolVersion,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LoginLinkParams {}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LoginLinkResult {
    pub url: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TaskCreateParams {
    pub task_id: Id,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub group: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub forbidden_roots: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub owner_key: Option<OwnerKey>,
}

pub type TaskCreateResult = Task;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TaskAttachParams {
    pub task_id: Id,
    pub owner_key: OwnerKey,
}

pub type TaskAttachResult = Task;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TaskCloseParams {
    pub task_id: Id,
}

pub type TaskCloseResult = Task;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StateGetParams {}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StateGetResult {
    pub tasks: Vec<Task>,
    pub sessions: Vec<Session>,
    pub turns: Vec<Turn>,
    pub items: Vec<Item>,
    pub usage: Vec<Usage>,
    pub cursor: Decimal,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SessionOpenParams {
    pub task_id: Id,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning_effort: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub service_tier: Option<String>,
}

pub type SessionOpenResult = Session;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SessionFinishParams {
    pub task_id: Id,
    pub session_id: Id,
}

pub type SessionFinishResult = Session;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SessionCancelParams {
    pub task_id: Id,
    pub session_id: Id,
}

pub type SessionCancelResult = Session;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TurnStartParams {
    pub session_id: Id,
    pub input: String,
}

pub type TurnStartResult = Turn;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TurnAppendParams {
    pub session_id: Id,
    pub turn_id: Id,
    pub input: String,
}

pub type TurnAppendResult = Turn;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TurnInterruptParams {
    pub session_id: Id,
    pub turn_id: Id,
}

pub type TurnInterruptResult = Turn;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct InputAnswerParams {
    pub session_id: Id,
    pub turn_id: Id,
    pub request_id: Id,
    pub answer: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct InputAnswerResult {}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ApprovalListParams {
    pub task_id: Id,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ApprovalListResult {
    pub requests: Vec<ApprovalRequest>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ApprovalRenewParams {
    pub task_id: Id,
    pub approval_id: Id,
}

pub type ApprovalRenewResult = ApprovalRequest;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ApprovalDecideParams {
    pub task_id: Id,
    pub approval_id: Id,
    pub decision_id: Id,
    pub digest: Digest,
    pub choice: Choice,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub answer: Option<serde_json::Map<String, serde_json::Value>>,
}

pub type ApprovalDecideResult = ApprovalDecision;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UsageGetParams {
    pub session_id: Id,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UsageGetResult {
    pub usage: Vec<Usage>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EventsSubscribeParams {
    pub after_cursor: Decimal,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EventsSubscribeResult {}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct InputRequested {
    pub request_id: Id,
    pub task_id: Id,
    pub session_id: Id,
    pub turn_id: Id,
    pub question: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RpcError {
    pub code: i64,
    pub message: String,
    pub data: RpcErrorData,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RpcErrorData {
    pub reason: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub oldest_cursor: Option<Decimal>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub high_water: Option<Decimal>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ErrorReason {
    #[serde(rename = "unsupported_version")]
    UnsupportedVersion,
    #[serde(rename = "initialize_required")]
    InitializeRequired,
    #[serde(rename = "forbidden")]
    Forbidden,
    #[serde(rename = "not_found")]
    NotFound,
    #[serde(rename = "conflict")]
    Conflict,
    #[serde(rename = "cursor_expired")]
    CursorExpired,
    #[serde(rename = "digest_mismatch")]
    DigestMismatch,
    #[serde(rename = "approval_expired")]
    ApprovalExpired,
    #[serde(rename = "request_withdrawn")]
    RequestWithdrawn,
    #[serde(rename = "method_not_found")]
    MethodNotFound,
    #[serde(rename = "invalid_params")]
    InvalidParams,
    #[serde(rename = "internal_error")]
    InternalError,
}

impl ErrorReason {
    pub fn code(self) -> i64 {
        match self {
            Self::UnsupportedVersion => -32001,
            Self::InitializeRequired => -32002,
            Self::Forbidden => -32003,
            Self::NotFound => -32004,
            Self::Conflict => -32005,
            Self::CursorExpired => -32006,
            Self::DigestMismatch => -32007,
            Self::ApprovalExpired => -32008,
            Self::RequestWithdrawn => -32009,
            Self::MethodNotFound => -32601,
            Self::InvalidParams => -32602,
            Self::InternalError => -32603,
        }
    }
}
