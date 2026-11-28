import type {
  ApprovalRequest,
  Decimal,
  Event,
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

  private update(next: StoreSnapshot): void {
    this.snapshot = next;
    for (const listener of this.listeners) {
      listener();
    }
  }
}
