import { describe, expect, it } from "vitest";
import type {
  ApprovalRequest,
  Event,
  InputRequested,
  Item,
  Session,
  StateGetResult,
  Task,
  Turn,
  Usage,
} from "@qo/protocol";
import {
  DEFAULT_TASK_FILTERS,
  TASK_FILTERS_STORAGE_KEY,
  loadTaskFilters,
  saveTaskFilters,
} from "./filters.js";
import { zhCN } from "./i18n/zh-CN.js";
import {
  apply,
  applyApprovalRequested,
  applyInputRequested,
  applySnapshot,
  compareDecimal,
  createInitialState,
  getMostUrgentSession,
  getNeedsAttentionItems,
  groupVisibleTasks,
  usageKey,
} from "./state.js";

const T1 = "11111111-1111-4111-8111-111111111111";
const T2 = "22222222-2222-4222-8222-222222222222";
const T3 = "33333333-3333-4333-8333-333333333333";
const S1 = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa";
const S2 = "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb";
const S3 = "cccccccc-cccc-4ccc-8ccc-cccccccccccc";
const TURN1 = "dddddddd-dddd-4ddd-8ddd-dddddddddddd";
const ITEM_MSG = "eeeeeeee-eeee-4eee-8eee-eeeeeeeeeeee";
const ITEM_CMD = "ffffffff-ffff-4fff-8fff-ffffffffffff";
const APPROVAL1 = "12121212-1212-4121-8121-121212121212";
const APPROVAL2 = "34343434-3434-4343-8343-343434343434";
const INPUT1 = "56565656-5656-4565-8565-565656565656";
const DIGEST = "a".repeat(64);
const AT = "2026-11-28T00:00:00.000Z";

function makeTask(overrides: Partial<Task> = {}): Task {
  return {
    taskId: T1,
    name: "任务一",
    cwd: "/work/t1",
    forbiddenRoots: [],
    createdBy: "ui",
    state: "open",
    createdAt: AT,
    ...overrides,
  };
}

function makeSession(overrides: Partial<Session> = {}): Session {
  return {
    sessionId: S1,
    taskId: T1,
    harness: "mock",
    harnessVersion: "0.1.0",
    state: "idle",
    createdAt: AT,
    ...overrides,
  };
}

function makeTurn(overrides: Partial<Turn> = {}): Turn {
  return {
    turnId: TURN1,
    sessionId: S1,
    input: "hello",
    state: "running",
    startedAt: AT,
    ...overrides,
  };
}

function makeApproval(
  overrides: Partial<ApprovalRequest> = {},
): ApprovalRequest {
  return {
    approvalId: APPROVAL1,
    taskId: T1,
    sessionId: S1,
    turnId: TURN1,
    itemId: ITEM_CMD,
    kind: "command_execution",
    raw: {
      method: "item/commandExecution/requestApproval",
      params: { command: "echo hi" },
    },
    summary: "echo hi",
    digest: DIGEST,
    expiresAt: "2026-11-28T00:10:00.000Z",
    state: "pending",
    ...overrides,
  };
}

function makeInput(overrides: Partial<InputRequested> = {}): InputRequested {
  return {
    requestId: INPUT1,
    taskId: T1,
    sessionId: S1,
    turnId: TURN1,
    questions: [
      {
        id: "q1",
        header: "格式",
        question: "选择输出格式",
        allowOther: false,
        secret: false,
      },
    ],
    ...overrides,
  };
}

describe("state reducer and deduplication", () => {
  it("deduplicates events with cursor <= applied cursor, including 64-bit decimal cursors", () => {
    expect(compareDecimal("9007199254740993", "9007199254740992")).toBe(1);
    expect(compareDecimal("9007199254740992", "9007199254740992")).toBe(0);
    expect(compareDecimal("9007199254740991", "9007199254740992")).toBe(-1);

    let state = applySnapshot(createInitialState(), {
      tasks: [makeTask({ name: "旧名" })],
      sessions: [],
      turns: [],
      items: [],
      usage: [],
      cursor: "9007199254740992",
    });

    const staleEvent: Event = {
      cursor: "9007199254740992",
      at: AT,
      taskId: T1,
      kind: "task.changed",
      payload: makeTask({ name: "不应覆盖" }),
    };
    const afterStale = apply(state, staleEvent);
    expect(afterStale).toBe(state);
    expect(afterStale.tasks[T1].name).toBe("旧名");

    const olderEvent: Event = {
      cursor: "10",
      at: AT,
      taskId: T1,
      kind: "task.changed",
      payload: makeTask({ name: "更旧也不应覆盖" }),
    };
    expect(apply(state, olderEvent)).toBe(state);

    const newerEvent: Event = {
      cursor: "9007199254740993",
      at: AT,
      taskId: T1,
      kind: "task.changed",
      payload: makeTask({ name: "新名" }),
    };
    state = apply(state, newerEvent);
    expect(state.cursor).toBe("9007199254740993");
    expect(state.tasks[T1].name).toBe("新名");
  });

  it("replaces entities by ID and appends item.delta for assistant messages and command output", () => {
    let state = createInitialState();

    const assistantStart: Item = {
      itemId: ITEM_MSG,
      turnId: TURN1,
      kind: "assistant_message",
      state: "running",
      startedAt: AT,
      content: "",
    };
    const commandStart: Item = {
      itemId: ITEM_CMD,
      turnId: TURN1,
      kind: "command_execution",
      state: "running",
      startedAt: AT,
      content: { command: "echo demo", output: "" },
    };

    state = apply(state, {
      cursor: "1",
      at: AT,
      taskId: T1,
      sessionId: S1,
      turnId: TURN1,
      kind: "item.changed",
      payload: assistantStart,
    });
    state = apply(state, {
      cursor: "2",
      at: AT,
      taskId: T1,
      sessionId: S1,
      turnId: TURN1,
      kind: "item.delta",
      payload: { itemId: ITEM_MSG, delta: "收到。" },
    });
    state = apply(state, {
      cursor: "3",
      at: AT,
      taskId: T1,
      sessionId: S1,
      turnId: TURN1,
      kind: "item.delta",
      payload: { itemId: ITEM_MSG, delta: "正在检查。" },
    });
    // 重复游标不重复追加
    state = apply(state, {
      cursor: "3",
      at: AT,
      taskId: T1,
      sessionId: S1,
      turnId: TURN1,
      kind: "item.delta",
      payload: { itemId: ITEM_MSG, delta: "正在检查。" },
    });

    const msgItem = state.items[ITEM_MSG];
    expect(msgItem.kind).toBe("assistant_message");
    if (msgItem.kind === "assistant_message") {
      expect(msgItem.content).toBe("收到。正在检查。");
    }

    state = apply(state, {
      cursor: "4",
      at: AT,
      taskId: T1,
      sessionId: S1,
      turnId: TURN1,
      kind: "item.changed",
      payload: commandStart,
    });
    state = apply(state, {
      cursor: "5",
      at: AT,
      taskId: T1,
      sessionId: S1,
      turnId: TURN1,
      kind: "item.delta",
      payload: { itemId: ITEM_CMD, delta: "line1\n" },
    });
    state = apply(state, {
      cursor: "6",
      at: AT,
      taskId: T1,
      sessionId: S1,
      turnId: TURN1,
      kind: "item.delta",
      payload: { itemId: ITEM_CMD, delta: "line2\n" },
    });

    const cmdRunning = state.items[ITEM_CMD];
    expect(cmdRunning.kind).toBe("command_execution");
    if (cmdRunning.kind === "command_execution") {
      expect(cmdRunning.content).toEqual({
        command: "echo demo",
        output: "line1\nline2\n",
      });
    }

    // item.changed 按 ID 整体替换
    state = apply(state, {
      cursor: "7",
      at: AT,
      taskId: T1,
      sessionId: S1,
      turnId: TURN1,
      kind: "item.changed",
      payload: {
        ...commandStart,
        state: "completed",
        endedAt: AT,
        content: {
          command: "echo demo",
          output: "line1\nline2\n",
          exitCode: 0,
        },
      },
    });
    const cmdDone = state.items[ITEM_CMD];
    if (cmdDone.kind === "command_execution") {
      expect(cmdDone.state).toBe("completed");
      expect(cmdDone.content.exitCode).toBe(0);
    }
  });

  it("replaces usage snapshots by ADR section 3 composite key", () => {
    let state = createInitialState();
    const baseUsage: Usage = {
      sessionId: S1,
      turnId: TURN1,
      model: "gpt-5",
      reasoningEffort: "high",
      serviceTier: "fast",
      source: "thread/tokenUsage/updated",
      counts: {
        totalTokens: "100",
        inputTokens: "60",
        cachedInputTokens: "0",
        outputTokens: "40",
        reasoningOutputTokens: null,
        cacheWriteInputTokens: null,
      },
      observedAt: AT,
    };

    state = apply(state, {
      cursor: "1",
      at: AT,
      taskId: T1,
      sessionId: S1,
      turnId: TURN1,
      kind: "usage.updated",
      payload: baseUsage,
    });

    // 同键替换
    const updatedSameKey: Usage = {
      ...baseUsage,
      counts: { ...baseUsage.counts, totalTokens: "250", outputTokens: "190" },
    };
    state = apply(state, {
      cursor: "2",
      at: AT,
      taskId: T1,
      sessionId: S1,
      turnId: TURN1,
      kind: "usage.updated",
      payload: updatedSameKey,
    });
    expect(Object.keys(state.usage)).toHaveLength(1);
    expect(state.usage[usageKey(baseUsage)].counts.totalTokens).toBe("250");

    // 不同来源分开存放，不覆盖、不相加
    const otherSource: Usage = {
      ...baseUsage,
      source: "account/usage/read",
      counts: { ...baseUsage.counts, totalTokens: "500" },
    };
    state = apply(state, {
      cursor: "3",
      at: AT,
      taskId: T1,
      sessionId: S1,
      turnId: TURN1,
      kind: "usage.updated",
      payload: otherSource,
    });
    expect(Object.keys(state.usage)).toHaveLength(2);
  });

  it("deduplicates private notifications and monotonically prunes stale ones", () => {
    let state = createInitialState();
    const approval = makeApproval();
    const input = makeInput();

    state = applyApprovalRequested(state, approval);
    const dedupApproval = applyApprovalRequested(state, { ...approval });
    expect(dedupApproval).toBe(state);
    expect(Object.keys(dedupApproval.approvals)).toEqual([APPROVAL1]);

    state = applyInputRequested(state, input);
    const dedupInput = applyInputRequested(state, { ...input });
    expect(dedupInput).toBe(state);
    expect(Object.keys(dedupInput.inputs)).toEqual([INPUT1]);

    // 快照保留仍处于非终态会话/条目的私有通知（如 initialize 后重发早于 state.get）
    const snapshot: StateGetResult = {
      tasks: [makeTask()],
      sessions: [makeSession({ state: "waiting_approval" })],
      turns: [makeTurn()],
      items: [
        {
          itemId: ITEM_CMD,
          turnId: TURN1,
          kind: "command_execution",
          state: "running",
          startedAt: AT,
          content: { command: "echo hi", output: "" },
        },
      ],
      usage: [],
      cursor: "5",
    };
    state = applySnapshot(state, snapshot);
    expect(Object.keys(state.approvals)).toEqual([APPROVAL1]);
    expect(Object.keys(state.inputs)).toEqual([INPUT1]);

    // 另一条目的新审批不受旧条目终态清理影响
    const approval2 = makeApproval({
      approvalId: APPROVAL2,
      itemId: ITEM_MSG,
    });
    state = applyApprovalRequested(state, approval2);
    state = apply(state, {
      cursor: "6",
      at: AT,
      taskId: T1,
      sessionId: S1,
      turnId: TURN1,
      kind: "item.changed",
      payload: {
        itemId: ITEM_CMD,
        turnId: TURN1,
        kind: "command_execution",
        state: "completed",
        startedAt: AT,
        endedAt: AT,
        content: { command: "echo hi", output: "hi\n", exitCode: 0 },
      },
    });
    expect(Object.keys(state.approvals)).toEqual([APPROVAL2]);

    // 会话取消后清理该会话剩余私有通知
    state = apply(state, {
      cursor: "7",
      at: AT,
      taskId: T1,
      sessionId: S1,
      kind: "session.changed",
      payload: makeSession({
        state: "cancelled",
        cancelledBy: "ui",
        endedAt: AT,
      }),
    });
    expect(Object.keys(state.approvals)).toHaveLength(0);
    expect(Object.keys(state.inputs)).toHaveLength(0);
  });
});

describe("task list grouping, urgency, attention queue, and filters", () => {
  it("selects urgent session with priority waiting_approval > waiting_input > running > others", () => {
    const idle = makeSession({ sessionId: "1", state: "idle" });
    const running = makeSession({ sessionId: "2", state: "running" });
    const input = makeSession({ sessionId: "3", state: "waiting_input" });
    const approval = makeSession({ sessionId: "4", state: "waiting_approval" });
    const failed = makeSession({ sessionId: "5", state: "failed" });

    expect(getMostUrgentSession([idle, failed, running])?.state).toBe(
      "running",
    );
    expect(getMostUrgentSession([idle, running, input])?.state).toBe(
      "waiting_input",
    );
    expect(getMostUrgentSession([running, input, approval])?.state).toBe(
      "waiting_approval",
    );
    expect(getMostUrgentSession([idle, failed])?.state).toBe("idle");
    expect(getMostUrgentSession([])).toBeNull();
  });

  it("groups tasks by group with ungrouped last, excludes external tasks from needs-attention, and formats tab title", () => {
    const snapshot: StateGetResult = {
      tasks: [
        makeTask({
          taskId: T1,
          name: "无分组任务",
          createdBy: "ui",
          createdAt: "2026-11-28T00:01:00.000Z",
        }),
        makeTask({
          taskId: T2,
          name: "外部任务",
          group: "Beta",
          createdBy: "external",
          createdAt: "2026-11-28T00:02:00.000Z",
        }),
        makeTask({
          taskId: T3,
          name: "已关闭任务",
          group: "Alpha",
          createdBy: "ui",
          state: "closed",
          createdAt: "2026-11-28T00:03:00.000Z",
          closedAt: "2026-11-28T00:04:00.000Z",
        }),
      ],
      sessions: [
        makeSession({
          sessionId: S1,
          taskId: T1,
          state: "waiting_input",
        }),
        makeSession({
          sessionId: S2,
          taskId: T2,
          state: "waiting_approval",
        }),
        makeSession({
          sessionId: S3,
          taskId: T1,
          state: "waiting_approval",
        }),
      ],
      turns: [],
      items: [],
      usage: [],
      cursor: "10",
    };

    const state = applySnapshot(createInitialState(), snapshot);

    // 仅自己的任务进入「需要你处理」，外部任务的 waiting_approval 不进入
    const attention = getNeedsAttentionItems(state);
    expect(attention.map((item) => item.session.sessionId)).toEqual([S3, S1]);
    expect(zhCN.app.documentTitle(attention.length)).toBe("(2) QO");
    expect(zhCN.app.documentTitle(0)).toBe("QO");

    // 分组排序：有分组在前，无分组在最后
    const allGroups = groupVisibleTasks(state, DEFAULT_TASK_FILTERS);
    expect(allGroups.map((g) => g.group)).toEqual(["Alpha", "Beta", null]);
    expect(allGroups[2].tasks[0].urgentSession?.sessionId).toBe(S3);

    // 筛选只影响任务列表显示
    const mineOpen = groupVisibleTasks(state, {
      hideClosed: true,
      onlyMine: true,
    });
    expect(mineOpen.map((g) => g.group)).toEqual([null]);
    expect(mineOpen[0].tasks.map((t) => t.task.taskId)).toEqual([T1]);
  });

  it("persists and recovers task filters safely when storage throws or contains invalid JSON", () => {
    const store = new Map<string, string>();
    const fakeStorage = {
      getItem: (key: string) => store.get(key) ?? null,
      setItem: (key: string, value: string) => {
        store.set(key, value);
      },
    };

    expect(loadTaskFilters(fakeStorage)).toEqual(DEFAULT_TASK_FILTERS);

    saveTaskFilters({ hideClosed: true, onlyMine: false }, fakeStorage);
    expect(loadTaskFilters(fakeStorage)).toEqual({
      hideClosed: true,
      onlyMine: false,
    });

    store.set(TASK_FILTERS_STORAGE_KEY, "{broken json");
    expect(loadTaskFilters(fakeStorage)).toEqual(DEFAULT_TASK_FILTERS);

    const throwingStorage = {
      getItem: () => {
        throw new Error("SecurityError");
      },
      setItem: () => {
        throw new Error("QuotaExceededError");
      },
    };
    expect(loadTaskFilters(throwingStorage)).toEqual(DEFAULT_TASK_FILTERS);
    expect(() =>
      saveTaskFilters({ hideClosed: true, onlyMine: true }, throwingStorage),
    ).not.toThrow();
  });
});
