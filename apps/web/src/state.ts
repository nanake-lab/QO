import type {
  ApprovalRequest,
  Decimal,
  Event,
  Id,
  InputRequested,
  Item,
  ItemState,
  Session,
  SessionState,
  StateGetResult,
  Task,
  Turn,
  TurnState,
  Usage,
} from "@qo/protocol";

export interface AppState {
  readonly cursor: Decimal | null;
  readonly tasks: Readonly<Record<Id, Task>>;
  readonly sessions: Readonly<Record<Id, Session>>;
  readonly turns: Readonly<Record<Id, Turn>>;
  readonly items: Readonly<Record<Id, Item>>;
  readonly usage: Readonly<Record<string, Usage>>;
  readonly approvals: Readonly<Record<Id, ApprovalRequest>>;
  readonly inputs: Readonly<Record<Id, InputRequested>>;
}

export interface TaskFilters {
  readonly hideClosed: boolean;
  readonly onlyMine: boolean;
}

export interface TaskSummary {
  readonly task: Task;
  readonly sessions: readonly Session[];
  readonly urgentSession: Session | null;
}

export interface TaskGroup {
  readonly group: string | null;
  readonly tasks: readonly TaskSummary[];
}

export interface NeedsAttentionItem {
  readonly task: Task;
  readonly session: Session;
  readonly approval: ApprovalRequest | null;
  readonly input: InputRequested | null;
}

export function createInitialState(): AppState {
  return {
    cursor: null,
    tasks: {},
    sessions: {},
    turns: {},
    items: {},
    usage: {},
    approvals: {},
    inputs: {},
  };
}

export function compareDecimal(a: Decimal, b: Decimal): -1 | 0 | 1 {
  const left = BigInt(a);
  const right = BigInt(b);
  if (left < right) return -1;
  if (left > right) return 1;
  return 0;
}

/**
 * ADR 第 3 节定义的用量键：按来源、会话及其统计维度（turnId、model、reasoningEffort、serviceTier）唯一替换。
 */
export function usageKey(usage: Usage): string {
  return JSON.stringify([
    usage.sessionId,
    usage.source,
    usage.turnId ?? null,
    usage.model ?? null,
    usage.reasoningEffort ?? null,
    usage.serviceTier ?? null,
  ]);
}

export function isTerminalSessionState(state: SessionState): boolean {
  return state === "completed" || state === "failed" || state === "cancelled";
}

function isTerminalTurnState(state: TurnState): boolean {
  return state === "completed" || state === "interrupted" || state === "failed";
}

function isTerminalItemState(state: ItemState): boolean {
  return state === "completed" || state === "failed" || state === "interrupted";
}

function filterRecord<T>(
  record: Readonly<Record<Id, T>>,
  keep: (value: T) => boolean,
): Readonly<Record<Id, T>> {
  let changed = false;
  const next: Record<Id, T> = {};
  for (const [id, value] of Object.entries(record)) {
    if (keep(value)) {
      next[id] = value;
    } else {
      changed = true;
    }
  }
  return changed ? next : record;
}

export function applySnapshot(
  state: AppState,
  snapshot: StateGetResult,
): AppState {
  const tasks: Record<Id, Task> = {};
  for (const task of snapshot.tasks) {
    tasks[task.taskId] = task;
  }

  const sessions: Record<Id, Session> = {};
  for (const session of snapshot.sessions) {
    sessions[session.sessionId] = session;
  }

  const turns: Record<Id, Turn> = {};
  for (const turn of snapshot.turns) {
    turns[turn.turnId] = turn;
  }

  const items: Record<Id, Item> = {};
  for (const item of snapshot.items) {
    items[item.itemId] = item;
  }

  const usage: Record<string, Usage> = {};
  for (const entry of snapshot.usage) {
    usage[usageKey(entry)] = entry;
  }

  const approvals = filterRecord(state.approvals, (request) => {
    const task = tasks[request.taskId];
    const session = sessions[request.sessionId];
    if (!task || task.state === "closed") return false;
    if (!session || isTerminalSessionState(session.state)) return false;
    if (request.turnId) {
      const turn = turns[request.turnId];
      if (turn && isTerminalTurnState(turn.state)) return false;
    }
    if (request.itemId) {
      const item = items[request.itemId];
      if (item && isTerminalItemState(item.state)) return false;
    }
    return true;
  });

  const inputs = filterRecord(state.inputs, (request) => {
    const task = tasks[request.taskId];
    const session = sessions[request.sessionId];
    if (!task || task.state === "closed") return false;
    if (!session || isTerminalSessionState(session.state)) return false;
    const turn = turns[request.turnId];
    if (turn && isTerminalTurnState(turn.state)) return false;
    return true;
  });

  return {
    cursor: snapshot.cursor,
    tasks,
    sessions,
    turns,
    items,
    usage,
    approvals,
    inputs,
  };
}

export function apply(state: AppState, event: Event): AppState {
  if (
    state.cursor !== null &&
    compareDecimal(event.cursor, state.cursor) <= 0
  ) {
    return state;
  }

  switch (event.kind) {
    case "task.changed": {
      const task = event.payload;
      const approvals =
        task.state === "closed"
          ? filterRecord(state.approvals, (r) => r.taskId !== task.taskId)
          : state.approvals;
      const inputs =
        task.state === "closed"
          ? filterRecord(state.inputs, (r) => r.taskId !== task.taskId)
          : state.inputs;
      return {
        ...state,
        cursor: event.cursor,
        tasks: { ...state.tasks, [task.taskId]: task },
        approvals,
        inputs,
      };
    }

    case "session.changed": {
      const session = event.payload;
      const terminal = isTerminalSessionState(session.state);
      const approvals = terminal
        ? filterRecord(
            state.approvals,
            (r) => r.sessionId !== session.sessionId,
          )
        : state.approvals;
      const inputs = terminal
        ? filterRecord(state.inputs, (r) => r.sessionId !== session.sessionId)
        : state.inputs;
      return {
        ...state,
        cursor: event.cursor,
        sessions: { ...state.sessions, [session.sessionId]: session },
        approvals,
        inputs,
      };
    }

    case "turn.changed": {
      const turn = event.payload;
      const terminal = isTerminalTurnState(turn.state);
      const approvals = terminal
        ? filterRecord(state.approvals, (r) => r.turnId !== turn.turnId)
        : state.approvals;
      const inputs = terminal
        ? filterRecord(state.inputs, (r) => r.turnId !== turn.turnId)
        : state.inputs;
      return {
        ...state,
        cursor: event.cursor,
        turns: { ...state.turns, [turn.turnId]: turn },
        approvals,
        inputs,
      };
    }

    case "item.changed": {
      const item = event.payload;
      const approvals = isTerminalItemState(item.state)
        ? filterRecord(state.approvals, (r) => r.itemId !== item.itemId)
        : state.approvals;
      return {
        ...state,
        cursor: event.cursor,
        items: { ...state.items, [item.itemId]: item },
        approvals,
      };
    }

    case "item.delta": {
      const { itemId, delta } = event.payload;
      const existing = state.items[itemId];
      if (!existing) {
        return { ...state, cursor: event.cursor };
      }
      if (existing.kind === "assistant_message") {
        const updated: Item = {
          ...existing,
          content: existing.content + delta,
        };
        return {
          ...state,
          cursor: event.cursor,
          items: { ...state.items, [itemId]: updated },
        };
      }
      if (existing.kind === "command_execution") {
        const updated: Item = {
          ...existing,
          content: {
            ...existing.content,
            output: existing.content.output + delta,
          },
        };
        return {
          ...state,
          cursor: event.cursor,
          items: { ...state.items, [itemId]: updated },
        };
      }
      return { ...state, cursor: event.cursor };
    }

    case "usage.updated": {
      const usage = event.payload;
      const key = usageKey(usage);
      return {
        ...state,
        cursor: event.cursor,
        usage: { ...state.usage, [key]: usage },
      };
    }
  }
}

export function applyApprovalRequested(
  state: AppState,
  request: ApprovalRequest,
): AppState {
  const existing = state.approvals[request.approvalId];
  if (
    existing &&
    existing.digest === request.digest &&
    existing.state === request.state &&
    existing.expiresAt === request.expiresAt &&
    existing.summary === request.summary
  ) {
    return state;
  }
  return {
    ...state,
    approvals: { ...state.approvals, [request.approvalId]: request },
  };
}

export function applyInputRequested(
  state: AppState,
  request: InputRequested,
): AppState {
  const existing = state.inputs[request.requestId];
  if (
    existing &&
    existing.taskId === request.taskId &&
    existing.sessionId === request.sessionId &&
    existing.turnId === request.turnId &&
    existing.questions.length === request.questions.length
  ) {
    return state;
  }
  return {
    ...state,
    inputs: { ...state.inputs, [request.requestId]: request },
  };
}

/**
 * 任务列表中展示最要紧的会话状态：等批准 > 等输入 > 在跑 > 其他。
 */
const SESSION_URGENCY: Readonly<Record<SessionState, number>> = {
  waiting_approval: 0,
  waiting_input: 1,
  running: 2,
  starting: 3,
  idle: 4,
  failed: 5,
  cancelled: 6,
  completed: 7,
};

export function compareSessionUrgency(a: Session, b: Session): number {
  const diff = SESSION_URGENCY[a.state] - SESSION_URGENCY[b.state];
  if (diff !== 0) return diff;
  if (a.createdAt !== b.createdAt) {
    return a.createdAt < b.createdAt ? -1 : 1;
  }
  return a.sessionId.localeCompare(b.sessionId);
}

export function getTaskSessions(state: AppState, taskId: Id): Session[] {
  return Object.values(state.sessions)
    .filter((session) => session.taskId === taskId)
    .sort((a, b) => {
      if (a.createdAt !== b.createdAt) {
        return a.createdAt < b.createdAt ? -1 : 1;
      }
      return a.sessionId.localeCompare(b.sessionId);
    });
}

export function getMostUrgentSession(
  sessions: readonly Session[],
): Session | null {
  if (sessions.length === 0) return null;
  let best = sessions[0];
  for (let i = 1; i < sessions.length; i += 1) {
    if (compareSessionUrgency(sessions[i], best) < 0) {
      best = sessions[i];
    }
  }
  return best;
}

export function getNeedsAttentionItems(state: AppState): NeedsAttentionItem[] {
  const pendingApprovalsBySession = new Map<Id, ApprovalRequest>();
  for (const approval of Object.values(state.approvals)) {
    if (approval.state === "pending" || approval.state === "expired") {
      if (!pendingApprovalsBySession.has(approval.sessionId)) {
        pendingApprovalsBySession.set(approval.sessionId, approval);
      }
    }
  }

  const pendingInputsBySession = new Map<Id, InputRequested>();
  for (const input of Object.values(state.inputs)) {
    if (!pendingInputsBySession.has(input.sessionId)) {
      pendingInputsBySession.set(input.sessionId, input);
    }
  }

  const list: NeedsAttentionItem[] = [];
  for (const session of Object.values(state.sessions)) {
    if (
      session.state !== "waiting_approval" &&
      session.state !== "waiting_input"
    ) {
      continue;
    }
    const task = state.tasks[session.taskId];
    if (!task || task.createdBy !== "ui") {
      continue;
    }
    list.push({
      task,
      session,
      approval:
        session.state === "waiting_approval"
          ? (pendingApprovalsBySession.get(session.sessionId) ?? null)
          : null,
      input:
        session.state === "waiting_input"
          ? (pendingInputsBySession.get(session.sessionId) ?? null)
          : null,
    });
  }

  list.sort((a, b) => compareSessionUrgency(a.session, b.session));
  return list;
}

export function groupVisibleTasks(
  state: AppState,
  filters: TaskFilters,
): TaskGroup[] {
  const byGroup = new Map<string | null, TaskSummary[]>();

  for (const task of Object.values(state.tasks)) {
    if (filters.hideClosed && task.state === "closed") continue;
    if (filters.onlyMine && task.createdBy !== "ui") continue;

    const sessions = getTaskSessions(state, task.taskId);
    const urgentSession = getMostUrgentSession(sessions);
    const trimmedGroup = task.group?.trim();
    const key = trimmedGroup ? trimmedGroup : null;
    const bucket = byGroup.get(key) ?? [];
    bucket.push({ task, sessions, urgentSession });
    byGroup.set(key, bucket);
  }

  const groupKeys = [...byGroup.keys()].sort((a, b) => {
    if (a === null && b === null) return 0;
    if (a === null) return 1;
    if (b === null) return -1;
    return a.localeCompare(b, "zh-CN");
  });

  return groupKeys.map((group) => {
    const tasks = byGroup.get(group) ?? [];
    tasks.sort((a, b) => {
      if (a.task.createdAt !== b.task.createdAt) {
        return a.task.createdAt > b.task.createdAt ? -1 : 1;
      }
      return a.task.taskId.localeCompare(b.task.taskId);
    });
    return { group, tasks };
  });
}

export function getSessionTurns(state: AppState, sessionId: Id): Turn[] {
  return Object.values(state.turns)
    .filter((turn) => turn.sessionId === sessionId)
    .sort((a, b) => {
      if (a.startedAt !== b.startedAt) {
        return a.startedAt < b.startedAt ? -1 : 1;
      }
      return a.turnId.localeCompare(b.turnId);
    });
}

export function getTurnItems(state: AppState, turnId: Id): Item[] {
  return Object.values(state.items)
    .filter((item) => item.turnId === turnId)
    .sort((a, b) => {
      if (a.startedAt !== b.startedAt) {
        return a.startedAt < b.startedAt ? -1 : 1;
      }
      return a.itemId.localeCompare(b.itemId);
    });
}
