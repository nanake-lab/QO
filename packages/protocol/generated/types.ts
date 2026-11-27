// Generated from the QO protocol schema. Run pnpm generate to update.

export type Id = string;

export type Timestamp = string;

export type Decimal = string;

export type Digest = string;

export type OwnerKey = string;

export type ProtocolVersion = "qo.v1";

export type Creator = "ui" | "external";

export type CancelledBy = "owner" | "ui";

export type TaskState = "open" | "closed";

export type SessionState =
  | "starting"
  | "idle"
  | "running"
  | "waiting_input"
  | "waiting_approval"
  | "completed"
  | "failed"
  | "cancelled";

export type TurnState = "running" | "completed" | "interrupted" | "failed";

export type ItemState = "running" | "completed" | "failed" | "interrupted";

export type ApprovalState = "pending" | "decided" | "expired" | "withdrawn";

export type ApprovalKind =
  "command_execution" | "file_change" | "permissions" | "elicitation";

export type Choice = "allow_once" | "deny";

export type Delivery = "recorded" | "sent" | "unknown";

export interface Task {
  taskId: Id;
  name: string;
  group?: string;
  cwd: string;
  forbiddenRoots: Array<string>;
  createdBy: Creator;
  state: TaskState;
  createdAt: Timestamp;
  closedAt?: Timestamp;
}

export interface Session {
  sessionId: Id;
  taskId: Id;
  harness: string;
  model?: string;
  reasoningEffort?: string;
  serviceTier?: string;
  state: SessionState;
  error?: string;
  createdAt: Timestamp;
  cancelledBy?: CancelledBy;
  endedAt?: Timestamp;
}

export interface Turn {
  turnId: Id;
  sessionId: Id;
  input: string;
  state: TurnState;
  startedAt: Timestamp;
  endedAt?: Timestamp;
}

export interface CommandContent {
  command: string;
  output: string;
  exitCode?: number;
}

export interface FileContent {
  path: string;
  diff: string;
}

export type Item =
  | {
      itemId: Id;
      turnId: Id;
      state: ItemState;
      startedAt: Timestamp;
      endedAt?: Timestamp;
      kind: "assistant_message";
      content: string;
    }
  | {
      itemId: Id;
      turnId: Id;
      state: ItemState;
      startedAt: Timestamp;
      endedAt?: Timestamp;
      kind: "command_execution";
      content: CommandContent;
    }
  | {
      itemId: Id;
      turnId: Id;
      state: ItemState;
      startedAt: Timestamp;
      endedAt?: Timestamp;
      kind: "file_change";
      content: FileContent;
    };

export interface RawRequest {
  method: string;
  params: Record<string, unknown>;
  context?: Record<string, unknown>;
}

export interface ApprovalRequest {
  approvalId: Id;
  taskId: Id;
  sessionId: Id;
  turnId?: Id;
  itemId?: Id;
  kind: ApprovalKind;
  raw: RawRequest;
  summary: string;
  digest: Digest;
  expiresAt: Timestamp;
  state: ApprovalState;
}

export interface ApprovalDecision {
  decisionId: Id;
  approvalId: Id;
  digest: Digest;
  choice: Choice;
  answer?: Record<string, unknown>;
  decidedAt: Timestamp;
  delivery: Delivery;
}

export type UsageCount = string | null;

export interface UsageCounts {
  totalTokens: UsageCount;
  inputTokens: UsageCount;
  cachedInputTokens: UsageCount;
  outputTokens: UsageCount;
  reasoningOutputTokens: UsageCount;
  cacheWriteInputTokens: UsageCount;
}

export interface Usage {
  sessionId: Id;
  turnId?: Id;
  model?: string;
  reasoningEffort?: string;
  serviceTier?: string;
  source: string;
  counts: UsageCounts;
  observedAt: Timestamp;
}

export interface ItemDelta {
  itemId: Id;
  delta: string;
}

export type Event =
  | {
      cursor: string;
      at: Timestamp;
      taskId: Id;
      sessionId?: Id;
      turnId?: Id;
      kind: "task.changed";
      payload: Task;
    }
  | {
      cursor: string;
      at: Timestamp;
      taskId: Id;
      sessionId?: Id;
      turnId?: Id;
      kind: "session.changed";
      payload: Session;
    }
  | {
      cursor: string;
      at: Timestamp;
      taskId: Id;
      sessionId?: Id;
      turnId?: Id;
      kind: "turn.changed";
      payload: Turn;
    }
  | {
      cursor: string;
      at: Timestamp;
      taskId: Id;
      sessionId?: Id;
      turnId?: Id;
      kind: "item.changed";
      payload: Item;
    }
  | {
      cursor: string;
      at: Timestamp;
      taskId: Id;
      sessionId?: Id;
      turnId?: Id;
      kind: "item.delta";
      payload: ItemDelta;
    }
  | {
      cursor: string;
      at: Timestamp;
      taskId: Id;
      sessionId?: Id;
      turnId?: Id;
      kind: "usage.updated";
      payload: Usage;
    };

export interface InitializeParams {
  versions: [string, ...string[]];
}

export interface InitializeResult {
  version: ProtocolVersion;
}

export type LoginLinkParams = Record<string, never>;

export interface LoginLinkResult {
  url: string;
}

export interface TaskCreateParams {
  taskId: Id;
  name: string;
  group?: string;
  cwd?: string;
  forbiddenRoots?: Array<string>;
  ownerKey?: OwnerKey;
}

export type TaskCreateResult = Task;

export interface TaskAttachParams {
  taskId: Id;
  ownerKey: OwnerKey;
}

export type TaskAttachResult = Task;

export interface TaskCloseParams {
  taskId: Id;
}

export type TaskCloseResult = Task;

export type StateGetParams = Record<string, never>;

export interface StateGetResult {
  tasks: Array<Task>;
  sessions: Array<Session>;
  turns: Array<Turn>;
  items: Array<Item>;
  usage: Array<Usage>;
  cursor: Decimal;
}

export interface SessionOpenParams {
  taskId: Id;
  model?: string;
  reasoningEffort?: string;
  serviceTier?: string;
}

export type SessionOpenResult = Session;

export interface SessionFinishParams {
  taskId: Id;
  sessionId: Id;
}

export type SessionFinishResult = Session;

export interface SessionCancelParams {
  taskId: Id;
  sessionId: Id;
}

export type SessionCancelResult = Session;

export interface TurnStartParams {
  sessionId: Id;
  input: string;
}

export type TurnStartResult = Turn;

export interface TurnAppendParams {
  sessionId: Id;
  turnId: Id;
  input: string;
}

export type TurnAppendResult = Turn;

export interface TurnInterruptParams {
  sessionId: Id;
  turnId: Id;
}

export type TurnInterruptResult = Turn;

export interface InputAnswerParams {
  sessionId: Id;
  turnId: Id;
  requestId: Id;
  answer: string;
}

export type InputAnswerResult = Record<string, never>;

export interface ApprovalListParams {
  taskId: Id;
}

export interface ApprovalListResult {
  requests: Array<ApprovalRequest>;
}

export interface ApprovalRenewParams {
  taskId: Id;
  approvalId: Id;
}

export type ApprovalRenewResult = ApprovalRequest;

export interface ApprovalDecideParams {
  taskId: Id;
  approvalId: Id;
  decisionId: Id;
  digest: Digest;
  choice: Choice;
  answer?: Record<string, unknown>;
}

export type ApprovalDecideResult = ApprovalDecision;

export interface UsageGetParams {
  sessionId: Id;
}

export interface UsageGetResult {
  usage: Array<Usage>;
}

export interface EventsSubscribeParams {
  afterCursor: Decimal;
}

export type EventsSubscribeResult = Record<string, never>;

export interface InputRequested {
  requestId: Id;
  taskId: Id;
  sessionId: Id;
  turnId: Id;
  question: string;
}

export type RpcError =
  | { code: -32001; message: string; data: { reason: "unsupported_version" } }
  | { code: -32002; message: string; data: { reason: "initialize_required" } }
  | { code: -32003; message: string; data: { reason: "forbidden" } }
  | { code: -32004; message: string; data: { reason: "not_found" } }
  | { code: -32005; message: string; data: { reason: "conflict" } }
  | {
      code: -32006;
      message: string;
      data: {
        reason: "cursor_expired";
        oldestCursor: Decimal;
        highWater: Decimal;
      };
    }
  | { code: -32007; message: string; data: { reason: "digest_mismatch" } }
  | { code: -32008; message: string; data: { reason: "approval_expired" } }
  | { code: -32009; message: string; data: { reason: "request_withdrawn" } }
  | { code: -32601; message: string; data: { reason: "method_not_found" } }
  | { code: -32602; message: string; data: { reason: "invalid_params" } }
  | { code: -32603; message: string; data: { reason: "internal_error" } };

export interface RpcErrorData {
  reason: string;
  oldestCursor?: Decimal;
  highWater?: Decimal;
}
