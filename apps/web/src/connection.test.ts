import { describe, expect, it } from "vitest";
import type {
  ApprovalRequest,
  Event,
  InputRequested,
  StateGetResult,
  Task,
} from "@qo/protocol";
import {
  INITIAL_RECONNECT_DELAY_MS,
  MAX_RECONNECT_DELAY_MS,
  QoConnection,
  computeReconnectDelay,
  exchangeLoginCode,
  type ConnectionStatus,
  type ServerProbeResult,
  type SocketLike,
} from "./connection.js";
import { QoRpcError } from "./protocol.js";
import { QoStore } from "./store.js";

const T1 = "11111111-1111-4111-8111-111111111111";
const S1 = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa";
const TURN1 = "dddddddd-dddd-4ddd-8ddd-dddddddddddd";
const APPROVAL1 = "12121212-1212-4121-8121-121212121212";
const INPUT1 = "56565656-5656-4565-8565-565656565656";
const DIGEST = "b".repeat(64);
const AT = "2026-11-28T00:00:00.000Z";

interface RpcRequestFrame {
  jsonrpc: string;
  id: number;
  method: string;
  params: Record<string, unknown>;
}

class MockSocket implements SocketLike {
  readyState = 0;
  onopen: ((event: unknown) => void) | null = null;
  onmessage: ((event: { data: unknown }) => void) | null = null;
  onerror: ((event: unknown) => void) | null = null;
  onclose: ((event: unknown) => void) | null = null;
  readonly sent: RpcRequestFrame[] = [];
  closed = false;

  send(data: string): void {
    this.sent.push(JSON.parse(data) as RpcRequestFrame);
  }

  close(): void {
    if (this.closed) return;
    this.closed = true;
    this.readyState = 3;
    this.onclose?.({});
  }

  emitOpen(): void {
    this.readyState = 1;
    this.onopen?.({});
  }

  emitClose(event: unknown = {}): void {
    if (this.closed) return;
    this.closed = true;
    this.readyState = 3;
    this.onclose?.(event);
  }

  emitMessage(frame: unknown): void {
    this.onmessage?.({ data: JSON.stringify(frame) });
  }

  replyOk(id: number, result: unknown): void {
    this.emitMessage({ jsonrpc: "2.0", id, result });
  }

  replyError(
    id: number,
    code: number,
    reason: string,
    extra: Record<string, unknown> = {},
  ): void {
    this.emitMessage({
      jsonrpc: "2.0",
      id,
      error: {
        code,
        message: reason,
        data: { reason, ...extra },
      },
    });
  }

  pushNotification(method: string, params: unknown): void {
    this.emitMessage({ jsonrpc: "2.0", method, params });
  }
}

class ManualTimers {
  private nextHandle = 1;
  private readonly timers = new Map<number, { fn: () => void; ms: number }>();
  readonly scheduledDelays: number[] = [];

  schedule = (fn: () => void, ms: number): number => {
    const id = this.nextHandle;
    this.nextHandle += 1;
    this.scheduledDelays.push(ms);
    this.timers.set(id, { fn, ms });
    return id;
  };

  cancel = (handle: unknown): void => {
    this.timers.delete(handle as number);
  };

  pendingCount(): number {
    return this.timers.size;
  }

  runNext(): void {
    const first = this.timers.entries().next().value;
    if (!first) throw new Error("no pending timer");
    const [id, entry] = first;
    this.timers.delete(id);
    entry.fn();
  }
}

async function flushMicrotasks(): Promise<void> {
  for (let i = 0; i < 6; i += 1) {
    await Promise.resolve();
  }
}

function sampleTask(name = "初始任务"): Task {
  return {
    taskId: T1,
    name,
    cwd: "/work/t1",
    forbiddenRoots: [],
    createdBy: "ui",
    state: "open",
    createdAt: AT,
  };
}

function sampleSnapshot(cursor: string, taskName = "初始任务"): StateGetResult {
  return {
    tasks: [sampleTask(taskName)],
    sessions: [
      {
        sessionId: S1,
        taskId: T1,
        harness: "mock",
        harnessVersion: "0.1.0",
        state: "waiting_approval",
        createdAt: AT,
      },
    ],
    turns: [
      {
        turnId: TURN1,
        sessionId: S1,
        input: "scenario:approval",
        state: "running",
        startedAt: AT,
      },
    ],
    items: [],
    usage: [],
    cursor,
  };
}

function sampleApproval(): ApprovalRequest {
  return {
    approvalId: APPROVAL1,
    taskId: T1,
    sessionId: S1,
    turnId: TURN1,
    kind: "command_execution",
    raw: {
      method: "item/commandExecution/requestApproval",
      params: { command: "echo hi" },
    },
    summary: "echo hi",
    digest: DIGEST,
    expiresAt: "2026-11-28T00:10:00.000Z",
    state: "pending",
  };
}

function sampleInput(): InputRequested {
  return {
    requestId: INPUT1,
    taskId: T1,
    sessionId: S1,
    turnId: TURN1,
    questions: [
      {
        id: "q1",
        header: "确认",
        question: "继续吗？",
        allowOther: false,
        secret: false,
      },
    ],
  };
}

describe("QoConnection", () => {
  it("doubles reconnect delay from 1s up to 10s", () => {
    expect(INITIAL_RECONNECT_DELAY_MS).toBe(1_000);
    expect(MAX_RECONNECT_DELAY_MS).toBe(10_000);
    expect(computeReconnectDelay(0)).toBe(1_000);
    expect(computeReconnectDelay(1)).toBe(2_000);
    expect(computeReconnectDelay(2)).toBe(4_000);
    expect(computeReconnectDelay(3)).toBe(8_000);
    expect(computeReconnectDelay(4)).toBe(10_000);
    expect(computeReconnectDelay(5)).toBe(10_000);
  });

  it("performs initialize -> state.get -> events.subscribe and resumes from applied cursor on reconnect", async () => {
    const store = new QoStore();
    const timers = new ManualTimers();
    const sockets: MockSocket[] = [];
    const statuses: ConnectionStatus[] = [];

    const conn = new QoConnection({
      url: "ws://127.0.0.1:47262/rpc",
      getAppliedCursor: store.getAppliedCursor,
      onSnapshot: store.onSnapshot,
      onEvent: store.onEvent,
      onApprovalRequested: store.onApprovalRequested,
      onInputRequested: store.onInputRequested,
      onStatusChange: (status) => {
        statuses.push(status);
        store.onStatusChange(status);
      },
      createSocket: () => {
        const socket = new MockSocket();
        sockets.push(socket);
        return socket;
      },
      probeServer: async () => "unreachable",
      exchangeLogin: async () => "none",
      scheduleTimer: timers.schedule,
      cancelTimer: timers.cancel,
    });

    conn.start();
    await flushMicrotasks();
    expect(sockets).toHaveLength(1);

    const first = sockets[0];
    first.emitOpen();
    expect(first.sent.map((m) => m.method)).toEqual(["initialize"]);
    expect(first.sent[0].params).toEqual({ versions: ["qo.v1"] });
    first.replyOk(first.sent[0].id, { version: "qo.v1" });
    await flushMicrotasks();

    expect(first.sent[1].method).toBe("state.get");
    first.replyOk(first.sent[1].id, sampleSnapshot("5"));
    await flushMicrotasks();

    expect(first.sent[2].method).toBe("events.subscribe");
    expect(first.sent[2].params).toEqual({ afterCursor: "5" });
    first.replyOk(first.sent[2].id, {});
    await flushMicrotasks();

    expect(conn.getStatus()).toEqual({ kind: "connected", version: "qo.v1" });

    // 应用实时事件推进已应用游标到 7
    const event6: Event = {
      cursor: "6",
      at: AT,
      taskId: T1,
      kind: "task.changed",
      payload: sampleTask("第 6 步"),
    };
    const event7: Event = {
      cursor: "7",
      at: AT,
      taskId: T1,
      kind: "task.changed",
      payload: sampleTask("第 7 步"),
    };
    first.pushNotification("event", event6);
    first.pushNotification("event", event7);
    expect(store.getAppliedCursor()).toBe("7");

    // 断线后按 1s -> 2s 退避重连
    first.emitClose();
    await flushMicrotasks();
    expect(timers.scheduledDelays).toEqual([1_000]);

    timers.runNext();
    expect(sockets).toHaveLength(2);
    // 第二次连不上（服务暂时不可达）-> 退避到 2s
    sockets[1].emitClose();
    await flushMicrotasks();
    expect(timers.scheduledDelays).toEqual([1_000, 2_000]);

    timers.runNext();
    expect(sockets).toHaveLength(3);
    const resumed = sockets[2];
    resumed.emitOpen();
    expect(resumed.sent[0].method).toBe("initialize");
    resumed.replyOk(resumed.sent[0].id, { version: "qo.v1" });
    await flushMicrotasks();

    // 重连直接用已应用游标 "7" 续订，不重复调用 state.get
    expect(resumed.sent.map((m) => m.method)).toEqual([
      "initialize",
      "events.subscribe",
    ]);
    expect(resumed.sent[1].params).toEqual({ afterCursor: "7" });
    resumed.replyOk(resumed.sent[1].id, {});
    await flushMicrotasks();

    // 重复的游标 7 被去重，游标 8 正常生效
    resumed.pushNotification("event", {
      ...event7,
      payload: sampleTask("重复游标不应覆盖"),
    });
    expect(store.getSnapshot().state.tasks[T1].name).toBe("第 7 步");

    resumed.pushNotification("event", {
      cursor: "8",
      at: AT,
      taskId: T1,
      kind: "task.changed",
      payload: sampleTask("续订后更新"),
    });
    expect(store.getAppliedCursor()).toBe("8");
    expect(store.getSnapshot().state.tasks[T1].name).toBe("续订后更新");

    // 重连成功后退避重置回 1s
    resumed.emitClose();
    await flushMicrotasks();
    expect(timers.scheduledDelays).toEqual([1_000, 2_000, 1_000]);
    expect(statuses.some((s) => s.kind === "reconnecting")).toBe(true);

    conn.dispose();
  });

  it("falls back to state.get snapshot and re-subscribes when cursor_expired occurs", async () => {
    const store = new QoStore();
    const timers = new ManualTimers();
    const sockets: MockSocket[] = [];

    // 预先已有旧游标 "7"
    store.onSnapshot(sampleSnapshot("7", "旧快照"));

    const conn = new QoConnection({
      url: "ws://127.0.0.1:47262/rpc",
      getAppliedCursor: store.getAppliedCursor,
      onSnapshot: store.onSnapshot,
      onEvent: store.onEvent,
      onApprovalRequested: store.onApprovalRequested,
      onInputRequested: store.onInputRequested,
      onStatusChange: store.onStatusChange,
      createSocket: () => {
        const socket = new MockSocket();
        sockets.push(socket);
        return socket;
      },
      probeServer: async () => "unreachable",
      exchangeLogin: async () => "none",
      scheduleTimer: timers.schedule,
      cancelTimer: timers.cancel,
    });

    conn.start();
    await flushMicrotasks();
    const socket = sockets[0];
    socket.emitOpen();

    socket.replyOk(socket.sent[0].id, { version: "qo.v1" });
    await flushMicrotasks();

    // 先尝试按旧游标续订
    expect(socket.sent[1].method).toBe("events.subscribe");
    expect(socket.sent[1].params).toEqual({ afterCursor: "7" });

    // 服务端返回 cursor_expired
    socket.replyError(socket.sent[1].id, -32006, "cursor_expired", {
      oldestCursor: "50",
      highWater: "60",
    });
    await flushMicrotasks();

    // 自动走 state.get 拉最新快照，再按新快照游标订阅
    expect(socket.sent[2].method).toBe("state.get");
    socket.replyOk(socket.sent[2].id, sampleSnapshot("60", "新快照"));
    await flushMicrotasks();

    expect(store.getAppliedCursor()).toBe("60");
    expect(store.getSnapshot().state.tasks[T1].name).toBe("新快照");
    expect(socket.sent[3].method).toBe("events.subscribe");
    expect(socket.sent[3].params).toEqual({ afterCursor: "60" });

    socket.replyOk(socket.sent[3].id, {});
    await flushMicrotasks();
    expect(conn.getStatus()).toEqual({ kind: "connected", version: "qo.v1" });

    conn.dispose();
  });

  it("deduplicates private notifications resent by the server across reconnects", async () => {
    const store = new QoStore();
    const timers = new ManualTimers();
    const sockets: MockSocket[] = [];

    const conn = new QoConnection({
      url: "ws://127.0.0.1:47262/rpc",
      getAppliedCursor: store.getAppliedCursor,
      onSnapshot: store.onSnapshot,
      onEvent: store.onEvent,
      onApprovalRequested: store.onApprovalRequested,
      onInputRequested: store.onInputRequested,
      onStatusChange: store.onStatusChange,
      createSocket: () => {
        const socket = new MockSocket();
        sockets.push(socket);
        return socket;
      },
      probeServer: async () => "unreachable",
      exchangeLogin: async () => "none",
      scheduleTimer: timers.schedule,
      cancelTimer: timers.cancel,
    });

    conn.start();
    await flushMicrotasks();
    const s1 = sockets[0];
    s1.emitOpen();
    s1.replyOk(s1.sent[0].id, { version: "qo.v1" });
    // 服务在 initialize 后重发私有通知（甚至早于 state.get 完成）
    s1.pushNotification("approval.requested", sampleApproval());
    s1.pushNotification("input.requested", sampleInput());
    await flushMicrotasks();

    s1.replyOk(s1.sent[1].id, sampleSnapshot("5"));
    await flushMicrotasks();
    s1.replyOk(s1.sent[2].id, {});
    await flushMicrotasks();

    expect(Object.keys(store.getSnapshot().state.approvals)).toEqual([
      APPROVAL1,
    ]);
    expect(Object.keys(store.getSnapshot().state.inputs)).toEqual([INPUT1]);

    // 断线重连后服务再次重发同一批私有通知
    s1.emitClose();
    await flushMicrotasks();
    timers.runNext();

    const s2 = sockets[1];
    s2.emitOpen();
    s2.replyOk(s2.sent[0].id, { version: "qo.v1" });
    s2.pushNotification("approval.requested", sampleApproval());
    s2.pushNotification("input.requested", sampleInput());
    await flushMicrotasks();
    s2.replyOk(s2.sent[1].id, {});
    await flushMicrotasks();

    expect(Object.keys(store.getSnapshot().state.approvals)).toEqual([
      APPROVAL1,
    ]);
    expect(Object.keys(store.getSnapshot().state.inputs)).toEqual([INPUT1]);

    conn.dispose();
  });

  it("stops retrying and reports auth_expired when upgrade receives 403 (server alive)", async () => {
    const store = new QoStore();
    const timers = new ManualTimers();
    const sockets: MockSocket[] = [];
    let probeResult: ServerProbeResult = "alive";

    const conn = new QoConnection({
      url: "ws://127.0.0.1:47262/rpc",
      getAppliedCursor: store.getAppliedCursor,
      onSnapshot: store.onSnapshot,
      onEvent: store.onEvent,
      onApprovalRequested: store.onApprovalRequested,
      onInputRequested: store.onInputRequested,
      onStatusChange: store.onStatusChange,
      createSocket: () => {
        const socket = new MockSocket();
        sockets.push(socket);
        return socket;
      },
      probeServer: async () => probeResult,
      exchangeLogin: async () => "none",
      scheduleTimer: timers.schedule,
      cancelTimer: timers.cancel,
    });

    conn.start();
    await flushMicrotasks();
    expect(sockets).toHaveLength(1);

    // 升级失败（未触发 open 就 close），同时同源探测表明服务在线 -> 403 cookie 失效
    sockets[0].emitClose();
    await flushMicrotasks();

    expect(conn.getStatus()).toEqual({
      kind: "auth_expired",
      cause: "session_expired",
    });
    expect(timers.pendingCount()).toBe(0);
    expect(timers.scheduledDelays).toEqual([]);
    expect(sockets).toHaveLength(1);

    probeResult = "unreachable";
    conn.dispose();
  });

  it("matches concurrent RPC requests by id and rejects with QoRpcError", async () => {
    const store = new QoStore();
    const timers = new ManualTimers();
    const sockets: MockSocket[] = [];

    const conn = new QoConnection({
      url: "ws://127.0.0.1:47262/rpc",
      getAppliedCursor: store.getAppliedCursor,
      onSnapshot: store.onSnapshot,
      onEvent: store.onEvent,
      onApprovalRequested: store.onApprovalRequested,
      onInputRequested: store.onInputRequested,
      createSocket: () => {
        const socket = new MockSocket();
        sockets.push(socket);
        return socket;
      },
      exchangeLogin: async () => "none",
      scheduleTimer: timers.schedule,
      cancelTimer: timers.cancel,
    });

    conn.start();
    await flushMicrotasks();
    const socket = sockets[0];
    socket.emitOpen();
    socket.replyOk(socket.sent[0].id, { version: "qo.v1" });
    await flushMicrotasks();
    socket.replyOk(socket.sent[1].id, sampleSnapshot("1"));
    await flushMicrotasks();
    socket.replyOk(socket.sent[2].id, {});
    await flushMicrotasks();

    const p1 = conn.call("usage.get", { sessionId: S1 });
    const p2 = conn.call("task.close", { taskId: T1 });

    const req1 = socket.sent[3];
    const req2 = socket.sent[4];
    expect(req1.id).not.toBe(req2.id);

    // 乱序回复：先回复 req2 错误，再回复 req1 成功
    socket.replyError(req2.id, -32005, "conflict");
    socket.replyOk(req1.id, { usage: [] });

    await expect(p1).resolves.toEqual({ usage: [] });
    await expect(p2).rejects.toBeInstanceOf(QoRpcError);
    await expect(p2).rejects.toMatchObject({
      code: -32005,
      reason: "conflict",
    });

    conn.dispose();
  });

  it("strips login fragment before POST /auth/exchange, invokes fetch unbound, and halts without opening WS when link is invalid", async () => {
    const replaced: string[] = [];
    const calls: Array<{ url: string; body: string }> = [];

    const resultInvalid = await exchangeLoginCode({
      hash: "#login=deadbeef",
      pathname: "/",
      search: "",
      replaceUrl: (url) => replaced.push(url),
      fetchFn: async function (this: unknown, input, init) {
        if (this !== undefined) {
          throw new TypeError("Illegal invocation");
        }
        calls.push({
          url: String(input),
          body: String(init?.body ?? ""),
        });
        return new Response(null, { status: 403 });
      } as typeof fetch,
    });

    expect(replaced).toEqual(["/"]);
    expect(calls).toEqual([
      { url: "/auth/exchange", body: JSON.stringify({ code: "deadbeef" }) },
    ]);
    expect(resultInvalid).toBe("invalid");

    const store = new QoStore();
    const timers = new ManualTimers();
    let openedCount = 0;

    const conn = new QoConnection({
      url: "ws://127.0.0.1:47262/rpc",
      getAppliedCursor: store.getAppliedCursor,
      onSnapshot: store.onSnapshot,
      onEvent: store.onEvent,
      onApprovalRequested: store.onApprovalRequested,
      onInputRequested: store.onInputRequested,
      onStatusChange: store.onStatusChange,
      createSocket: () => {
        openedCount += 1;
        return new MockSocket();
      },
      exchangeLogin: async () => "invalid",
      scheduleTimer: timers.schedule,
      cancelTimer: timers.cancel,
    });

    conn.start();
    await flushMicrotasks();

    expect(openedCount).toBe(0);
    expect(timers.pendingCount()).toBe(0);
    expect(conn.getStatus()).toEqual({
      kind: "auth_expired",
      cause: "login_link_invalid",
    });

    conn.dispose();
  });

  it("retries in-flight client-ID write requests with identical ID and params after reconnect", async () => {
    const store = new QoStore();
    const timers = new ManualTimers();
    const sockets: MockSocket[] = [];

    const conn = new QoConnection({
      url: "ws://127.0.0.1:47262/rpc",
      getAppliedCursor: store.getAppliedCursor,
      onSnapshot: store.onSnapshot,
      onEvent: store.onEvent,
      onApprovalRequested: store.onApprovalRequested,
      onInputRequested: store.onInputRequested,
      onStatusChange: store.onStatusChange,
      createSocket: () => {
        const socket = new MockSocket();
        sockets.push(socket);
        return socket;
      },
      probeServer: async () => "unreachable",
      exchangeLogin: async () => "none",
      scheduleTimer: timers.schedule,
      cancelTimer: timers.cancel,
    });

    conn.start();
    await flushMicrotasks();
    const s1 = sockets[0];
    s1.emitOpen();
    s1.replyOk(s1.sent[0].id, { version: "qo.v1" });
    await flushMicrotasks();
    s1.replyOk(s1.sent[1].id, sampleSnapshot("5"));
    await flushMicrotasks();
    s1.replyOk(s1.sent[2].id, {});
    await flushMicrotasks();

    const createParams = { taskId: T1, name: "重试任务" };
    const openParams = { taskId: T1, sessionId: S1, model: "mock" };
    const startParams = { sessionId: S1, turnId: TURN1, input: "hello" };
    const appendId = "99999999-9999-4999-8999-999999999999";
    const appendParams = {
      sessionId: S1,
      turnId: TURN1,
      appendId,
      input: "more",
    };
    const decisionId = "88888888-8888-4888-8888-888888888888";
    const decideParams = {
      taskId: T1,
      approvalId: APPROVAL1,
      decisionId,
      digest: DIGEST,
      choice: "allow_once" as const,
    };

    const pCreate = conn.call("task.create", createParams);
    const pOpen = conn.call("session.open", openParams);
    const pStart = conn.call("turn.start", startParams);
    const pAppend = conn.call("turn.append", appendParams);
    const pDecide = conn.call("approval.decide", decideParams);
    const pClose = conn.call("task.close", { taskId: T1 });

    // 未收到回复前断线：非幂等调用立刻失败，全部带客户端 ID 的写操作排队等待重连重发
    s1.emitClose();
    await flushMicrotasks();
    await expect(pClose).rejects.toThrow("连接已断开");

    timers.runNext();
    const s2 = sockets[1];
    s2.emitOpen();
    s2.replyOk(s2.sent[0].id, { version: "qo.v1" });
    await flushMicrotasks();
    s2.replyOk(s2.sent[1].id, {});
    await flushMicrotasks();

    // 重连并续订完成后，按顺序以完全相同的 ID 与参数重发
    const resent = s2.sent.slice(2);
    expect(resent.map((r) => r.method)).toEqual([
      "task.create",
      "session.open",
      "turn.start",
      "turn.append",
      "approval.decide",
    ]);
    expect(resent[0].params).toEqual(createParams);
    expect(resent[1].params).toEqual(openParams);
    expect(resent[2].params).toEqual(startParams);
    expect(resent[3].params).toEqual(appendParams);
    expect(resent[4].params).toEqual(decideParams);

    s2.replyOk(resent[0].id, sampleTask("重试任务"));
    s2.replyOk(resent[1].id, {
      sessionId: S1,
      taskId: T1,
      harness: "mock",
      harnessVersion: "0.1.0",
      model: "mock",
      state: "idle",
      createdAt: AT,
    });
    s2.replyOk(resent[2].id, {
      turnId: TURN1,
      sessionId: S1,
      input: "hello",
      state: "running",
      startedAt: AT,
    });
    s2.replyOk(resent[3].id, {
      turnId: TURN1,
      sessionId: S1,
      input: "hello\nmore",
      state: "running",
      startedAt: AT,
    });
    s2.replyOk(resent[4].id, {
      decisionId,
      approvalId: APPROVAL1,
      digest: DIGEST,
      choice: "allow_once",
      decidedAt: AT,
      delivery: "sent",
    });

    await expect(pCreate).resolves.toMatchObject({ taskId: T1 });
    await expect(pOpen).resolves.toMatchObject({ sessionId: S1 });
    await expect(pStart).resolves.toMatchObject({ turnId: TURN1 });
    await expect(pAppend).resolves.toMatchObject({ input: "hello\nmore" });
    await expect(pDecide).resolves.toMatchObject({
      decisionId,
      delivery: "sent",
    });

    conn.dispose();
  });

  it("rejects queued idempotent writes when session becomes permanently auth_expired", async () => {
    const store = new QoStore();
    const timers = new ManualTimers();
    const sockets: MockSocket[] = [];

    const conn = new QoConnection({
      url: "ws://127.0.0.1:47262/rpc",
      getAppliedCursor: store.getAppliedCursor,
      onSnapshot: store.onSnapshot,
      onEvent: store.onEvent,
      onApprovalRequested: store.onApprovalRequested,
      onInputRequested: store.onInputRequested,
      onStatusChange: store.onStatusChange,
      createSocket: () => {
        const socket = new MockSocket();
        sockets.push(socket);
        return socket;
      },
      probeServer: async () => "alive",
      exchangeLogin: async () => "none",
      scheduleTimer: timers.schedule,
      cancelTimer: timers.cancel,
    });

    conn.start();
    await flushMicrotasks();

    // 在连接尚未就绪时提交幂等写请求，若握手遇到 403 则不应永久挂起
    const pendingCreate = conn.call("task.create", {
      taskId: T1,
      name: "不会卡死",
    });
    sockets[0].emitClose();
    await flushMicrotasks();

    await expect(pendingCreate).rejects.toThrow("登录状态已失效");
    expect(conn.getStatus()).toEqual({
      kind: "auth_expired",
      cause: "session_expired",
    });

    conn.dispose();
  });
});
