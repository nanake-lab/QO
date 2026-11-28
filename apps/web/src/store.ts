import type {
  ApprovalDecideResult,
  ApprovalRequest,
  Decimal,
  Event,
  Id,
  InputRequested,
  StateGetResult,
} from "@qo/protocol";
import type { ConnectionStatus } from "./connection.js";
import {
  apply,
  applyApprovalRequested,
  applyInputRequested,
  applySnapshot,
  createInitialState,
  markApprovalExpired,
  markApprovalWithdrawn,
  markInputWithdrawn,
  recordApprovalDecision,
  removeInputRequest,
  replaceRenewedApproval,
  setApprovalDeciding,
  setInputAnswering,
  syncTaskApprovals,
  type AppState,
} from "./state.js";

export interface StoreSnapshot {
  readonly state: AppState;
  readonly connectionStatus: ConnectionStatus;
}

export class QoStore {
  private snapshot: StoreSnapshot = {
    state: createInitialState(),
    connectionStatus: { kind: "connecting" },
  };
  private readonly listeners = new Set<() => void>();

  getSnapshot = (): StoreSnapshot => this.snapshot;

  getAppliedCursor = (): Decimal | null => this.snapshot.state.cursor;

  subscribe = (listener: () => void): (() => void) => {
    this.listeners.add(listener);
    return () => {
      this.listeners.delete(listener);
    };
  };

  onSnapshot = (data: StateGetResult): void => {
    const nextState = applySnapshot(this.snapshot.state, data);
    this.update({ ...this.snapshot, state: nextState });
  };

  onEvent = (event: Event): void => {
    const nextState = apply(this.snapshot.state, event);
    if (nextState === this.snapshot.state) return;
    this.update({ ...this.snapshot, state: nextState });
  };

  onApprovalRequested = (request: ApprovalRequest): void => {
    const nextState = applyApprovalRequested(this.snapshot.state, request);
    if (nextState === this.snapshot.state) return;
    this.update({ ...this.snapshot, state: nextState });
  };

  onInputRequested = (request: InputRequested): void => {
    const nextState = applyInputRequested(this.snapshot.state, request);
    if (nextState === this.snapshot.state) return;
    this.update({ ...this.snapshot, state: nextState });
  };

  onStatusChange = (connectionStatus: ConnectionStatus): void => {
    this.update({ ...this.snapshot, connectionStatus });
  };

  setApprovalDeciding = (approvalId: Id, deciding: boolean): void => {
    const nextState = setApprovalDeciding(
      this.snapshot.state,
      approvalId,
      deciding,
    );
    if (nextState === this.snapshot.state) return;
    this.update({ ...this.snapshot, state: nextState });
  };

  recordApprovalDecision = (
    request: ApprovalRequest,
    decision: ApprovalDecideResult,
  ): void => {
    const nextState = recordApprovalDecision(
      this.snapshot.state,
      request,
      decision,
    );
    this.update({ ...this.snapshot, state: nextState });
  };

  markApprovalExpired = (approvalId: Id): void => {
    const nextState = markApprovalExpired(this.snapshot.state, approvalId);
    if (nextState === this.snapshot.state) return;
    this.update({ ...this.snapshot, state: nextState });
  };

  replaceRenewedApproval = (
    oldApprovalId: Id,
    renewed: ApprovalRequest,
  ): void => {
    const nextState = replaceRenewedApproval(
      this.snapshot.state,
      oldApprovalId,
      renewed,
    );
    this.update({ ...this.snapshot, state: nextState });
  };

  markApprovalWithdrawn = (request: ApprovalRequest): void => {
    const nextState = markApprovalWithdrawn(this.snapshot.state, request);
    this.update({ ...this.snapshot, state: nextState });
  };

  syncTaskApprovals = (
    taskId: Id,
    requests: readonly ApprovalRequest[],
  ): void => {
    const nextState = syncTaskApprovals(this.snapshot.state, taskId, requests);
    this.update({ ...this.snapshot, state: nextState });
  };

  setInputAnswering = (requestId: Id, answering: boolean): void => {
    const nextState = setInputAnswering(
      this.snapshot.state,
      requestId,
      answering,
    );
    if (nextState === this.snapshot.state) return;
    this.update({ ...this.snapshot, state: nextState });
  };

  removeInputRequest = (requestId: Id): void => {
    const nextState = removeInputRequest(this.snapshot.state, requestId);
    if (nextState === this.snapshot.state) return;
    this.update({ ...this.snapshot, state: nextState });
  };

  markInputWithdrawn = (request: InputRequested): void => {
    const nextState = markInputWithdrawn(this.snapshot.state, request);
    this.update({ ...this.snapshot, state: nextState });
  };

  private update(next: StoreSnapshot): void {
    this.snapshot = next;
    for (const listener of this.listeners) {
      listener();
    }
  }
}
