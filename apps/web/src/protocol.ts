import type {
  ApprovalDecideParams,
  ApprovalDecideResult,
  ApprovalGetParams,
  ApprovalGetResult,
  ApprovalListParams,
  ApprovalListResult,
  ApprovalRenewParams,
  ApprovalRenewResult,
  ApprovalRequest,
  Event,
  EventsSubscribeParams,
  EventsSubscribeResult,
  InitializeParams,
  InitializeResult,
  InputAnswerParams,
  InputAnswerResult,
  InputRequested,
  LoginLinkParams,
  LoginLinkResult,
  RpcError,
  RpcErrorData,
  SessionCancelParams,
  SessionCancelResult,
  SessionFinishParams,
  SessionFinishResult,
  SessionOpenParams,
  SessionOpenResult,
  StateGetParams,
  StateGetResult,
  TaskAttachParams,
  TaskAttachResult,
  TaskCloseParams,
  TaskCloseResult,
  TaskCreateParams,
  TaskCreateResult,
  TurnAppendParams,
  TurnAppendResult,
  TurnInterruptParams,
  TurnInterruptResult,
  TurnStartParams,
  TurnStartResult,
  UsageGetParams,
  UsageGetResult,
} from "@qo/protocol";

export interface ProtocolMethods {
  initialize: { params: InitializeParams; result: InitializeResult };
  "ui.loginLink": { params: LoginLinkParams; result: LoginLinkResult };
  "task.create": { params: TaskCreateParams; result: TaskCreateResult };
  "task.attach": { params: TaskAttachParams; result: TaskAttachResult };
  "task.close": { params: TaskCloseParams; result: TaskCloseResult };
  "state.get": { params: StateGetParams; result: StateGetResult };
  "session.open": { params: SessionOpenParams; result: SessionOpenResult };
  "session.finish": {
    params: SessionFinishParams;
    result: SessionFinishResult;
  };
  "session.cancel": {
    params: SessionCancelParams;
    result: SessionCancelResult;
  };
  "turn.start": { params: TurnStartParams; result: TurnStartResult };
  "turn.append": { params: TurnAppendParams; result: TurnAppendResult };
  "turn.interrupt": {
    params: TurnInterruptParams;
    result: TurnInterruptResult;
  };
  "input.answer": { params: InputAnswerParams; result: InputAnswerResult };
  "approval.list": { params: ApprovalListParams; result: ApprovalListResult };
  "approval.renew": {
    params: ApprovalRenewParams;
    result: ApprovalRenewResult;
  };
  "approval.decide": {
    params: ApprovalDecideParams;
    result: ApprovalDecideResult;
  };
  "approval.get": { params: ApprovalGetParams; result: ApprovalGetResult };
  "usage.get": { params: UsageGetParams; result: UsageGetResult };
  "events.subscribe": {
    params: EventsSubscribeParams;
    result: EventsSubscribeResult;
  };
}

export type ProtocolMethodName = keyof ProtocolMethods;

export interface ProtocolNotifications {
  event: Event;
  "approval.requested": ApprovalRequest;
  "input.requested": InputRequested;
}

export type ProtocolNotificationName = keyof ProtocolNotifications;

export type RpcErrorReason = RpcError["data"]["reason"];

export class QoRpcError extends Error {
  readonly code: number;
  readonly reason: string;
  readonly data: RpcErrorData;

  constructor(error: { code: number; message: string; data: RpcErrorData }) {
    super(error.message);
    this.name = "QoRpcError";
    this.code = error.code;
    this.reason = error.data.reason;
    this.data = error.data;
  }
}

export function isQoRpcError(error: unknown): error is QoRpcError {
  return error instanceof QoRpcError;
}
