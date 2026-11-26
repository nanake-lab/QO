/// Generated from JSON Schema. Run pnpm generate to update.
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

pub type AttestationGenerateParams = HashMap<String, Option<serde_json::Value>>;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CommandExecutionRequestApprovalParams {
    /// Unique identifier for this specific approval callback.
    ///
    /// For regular shell/unified_exec approvals, this is null.
    ///
    /// For zsh-exec-bridge subcommand approvals, multiple callbacks can belong to one parent
    /// `itemId`, so `approvalId` is a distinct opaque callback id (a UUID) used to disambiguate
    /// routing. Stdin approvals also use a distinct callback id; inspect `kind` to distinguish
    /// them.
    pub approval_id: Option<String>,

    /// The command to be executed.
    pub command: Option<String>,

    /// Best-effort parsed command actions for friendly display.
    pub command_actions: Option<Vec<CommandAction>>,

    /// The command's working directory.
    pub cwd: Option<String>,

    /// Environment in which the command will run.
    pub environment_id: Option<String>,

    pub item_id: String,

    /// Kind of action under review. Defaults to `command` for older servers.
    pub kind: Option<CommandExecutionApprovalKind>,

    /// Optional context for a managed-network approval prompt.
    pub network_approval_context: Option<NetworkApprovalContext>,

    /// Optional proposed execpolicy amendment to allow similar commands without prompting.
    pub proposed_execpolicy_amendment: Option<Vec<String>>,

    /// Optional proposed network policy amendments (allow/deny host) for future requests.
    pub proposed_network_policy_amendments: Option<Vec<NetworkPolicyAmendment>>,

    /// Optional explanatory reason (e.g. request for network access).
    pub reason: Option<String>,

    /// Unix timestamp (in milliseconds) when this approval request started.
    pub started_at_ms: i64,

    pub thread_id: String,

    pub turn_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandAction {
    pub command: String,

    pub name: Option<String>,

    pub path: Option<String>,

    #[serde(rename = "type")]
    pub command_action_type: CommandActionType,

    pub query: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CommandActionType {
    #[serde(rename = "listFiles")]
    ListFiles,

    Read,

    Search,

    Unknown,
}

/// Kind of action under review. Defaults to `command` for older servers.
///
/// Distinguishes a command approval from input sent to an existing terminal.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CommandExecutionApprovalKind {
    Command,

    #[serde(rename = "writeStdin")]
    WriteStdin,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkApprovalContext {
    pub host: String,

    pub protocol: NetworkApprovalProtocol,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum NetworkApprovalProtocol {
    Http,

    Https,

    #[serde(rename = "socks5Tcp")]
    Socks5Tcp,

    #[serde(rename = "socks5Udp")]
    Socks5Udp,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkPolicyAmendment {
    pub action: NetworkPolicyRuleAction,

    pub host: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NetworkPolicyRuleAction {
    Allow,

    Deny,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileChangeRequestApprovalParams {
    /// [UNSTABLE] When set, the agent is asking the user to allow writes under this root for the
    /// remainder of the session (unclear if this is honored today).
    pub grant_root: Option<String>,

    pub item_id: String,

    /// Optional explanatory reason (e.g. request for extra write access).
    pub reason: Option<String>,

    /// Unix timestamp (in milliseconds) when this approval request started.
    pub started_at_ms: i64,

    pub thread_id: String,

    pub turn_id: String,
}

/// EXPERIMENTAL. Params sent with a request_user_input event.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolRequestUserInputParams {
    /// @deprecated Use `isBlocking` to decide whether the request should block.
    pub auto_resolution_ms: Option<i64>,

    pub is_blocking: bool,

    pub item_id: String,

    pub questions: Vec<ToolRequestUserInputQuestion>,

    pub thread_id: String,

    pub turn_id: String,
}

/// EXPERIMENTAL. Represents one request_user_input question and its required options.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolRequestUserInputQuestion {
    pub header: String,

    pub id: String,

    pub is_other: Option<bool>,

    pub is_secret: Option<bool>,

    pub options: Option<Vec<ToolRequestUserInputOption>>,

    pub question: String,
}

/// EXPERIMENTAL. Defines a single selectable option for request_user_input.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolRequestUserInputOption {
    pub description: String,

    pub label: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpServerElicitationRequestParams {
    pub server_name: String,

    pub thread_id: String,

    /// Active Codex turn when this elicitation was observed, if app-server could correlate one.
    ///
    /// This is nullable because MCP models elicitation as a standalone server-to-client request
    /// identified by the MCP server request id. It may be triggered during a turn, but turn
    /// context is app-server correlation rather than part of the protocol identity of the
    /// elicitation itself.
    pub turn_id: Option<String>,

    #[serde(rename = "_meta")]
    pub meta: Option<serde_json::Value>,

    pub message: String,

    pub mode: Mode,

    pub requested_schema: Option<serde_json::Value>,

    pub elicitation_id: Option<String>,

    pub url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Mode {
    Form,

    #[serde(rename = "openaiForm")]
    ModeOpenaiForm,

    #[serde(rename = "openai/form")]
    OpenaiForm,

    Url,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PermissionsRequestApprovalParams {
    pub cwd: String,

    pub environment_id: Option<String>,

    pub item_id: String,

    pub permissions: RequestPermissionProfile,

    pub reason: Option<String>,

    /// Unix timestamp (in milliseconds) when this approval request started.
    pub started_at_ms: i64,

    pub thread_id: String,

    pub turn_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RequestPermissionProfile {
    pub file_system: Option<AdditionalFileSystemPermissions>,

    pub network: Option<AdditionalNetworkPermissions>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AdditionalFileSystemPermissions {
    pub entries: Option<Vec<FileSystemSandboxEntry>>,

    pub glob_scan_max_depth: Option<i64>,

    /// This will be removed in favor of `entries`.
    pub read: Option<Vec<String>>,

    /// This will be removed in favor of `entries`.
    pub write: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileSystemSandboxEntry {
    pub access: FileSystemAccessMode,

    pub path: FileSystemPath,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FileSystemAccessMode {
    Deny,

    Read,

    Write,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileSystemPath {
    pub path: Option<String>,

    #[serde(rename = "type")]
    pub file_system_path_type: FileSystemPathType,

    pub pattern: Option<String>,

    pub value: Option<FileSystemSpecialPath>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FileSystemPathType {
    #[serde(rename = "glob_pattern")]
    GlobPattern,

    Path,

    Special,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileSystemSpecialPath {
    pub kind: Kind,

    pub subpath: Option<String>,

    pub path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Minimal,

    #[serde(rename = "project_roots")]
    ProjectRoots,

    Root,

    #[serde(rename = "slash_tmp")]
    SlashTmp,

    Tmpdir,

    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdditionalNetworkPermissions {
    pub enabled: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DynamicToolCallParams {
    pub arguments: Option<serde_json::Value>,

    pub call_id: String,

    pub namespace: Option<String>,

    pub thread_id: String,

    pub tool: String,

    pub turn_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatgptAuthTokensRefreshParams {
    /// Workspace/account identifier that Codex was previously using.
    ///
    /// Clients that manage multiple accounts/workspaces can use this as a hint to refresh the
    /// token for the correct workspace.
    ///
    /// This may be `null` when the prior auth state did not include a workspace identifier
    /// (`chatgpt_account_id`).
    pub previous_account_id: Option<String>,

    pub reason: ChatgptAuthTokensRefreshReason,
}

/// Codex attempted a backend request and received `401 Unauthorized`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChatgptAuthTokensRefreshReason {
    Unauthorized,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplyPatchApprovalParams {
    /// Use to correlate this with [codex_protocol::protocol::PatchApplyBeginEvent] and
    /// [codex_protocol::protocol::PatchApplyEndEvent].
    pub call_id: String,

    pub conversation_id: String,

    pub file_changes: HashMap<String, FileChange>,

    /// When set, the agent is asking the user to allow writes under this root for the remainder
    /// of the session (unclear if this is honored today).
    pub grant_root: Option<String>,

    /// Optional explanatory reason (e.g. request for extra write access).
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileChange {
    pub content: Option<String>,

    #[serde(rename = "type")]
    pub file_change_type: FileChangeType,

    pub move_path: Option<String>,

    pub unified_diff: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FileChangeType {
    Add,

    Delete,

    Update,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecCommandApprovalParams {
    /// Identifier for this specific approval callback.
    pub approval_id: Option<String>,

    /// Use to correlate this with [codex_protocol::protocol::ExecCommandBeginEvent] and
    /// [codex_protocol::protocol::ExecCommandEndEvent].
    pub call_id: String,

    pub command: Vec<String>,

    pub conversation_id: String,

    pub cwd: String,

    pub parsed_cmd: Vec<ParsedCommand>,

    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParsedCommand {
    pub cmd: String,

    pub name: Option<String>,

    /// (Best effort) Path to the file being read by the command. When possible, this is an
    /// absolute path, though when relative, it should be resolved against the `cwd`` that will
    /// be used to run the command to derive the absolute path.
    pub path: Option<String>,

    #[serde(rename = "type")]
    pub parsed_command_type: ParsedCommandType,

    pub query: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ParsedCommandType {
    #[serde(rename = "list_files")]
    ListFiles,

    Read,

    Search,

    Unknown,
}
