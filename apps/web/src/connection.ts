import type {
  ApprovalRequest,
  Decimal,
  Event,
  InputRequested,
  RpcErrorData,
  StateGetResult,
} from "@qo/protocol";
import {
  QoRpcError,
  isQoRpcError,
  type ProtocolMethodName,
  type ProtocolMethods,
} from "./protocol.js";

/**
 * 断线重连退避从 1 秒起步并按 2 倍递增，上限 10 秒：
 * 本机服务短暂抖动或慢连接被服务端断开时 1 秒即可恢复，
 * 服务停机时翻倍退避避免空转，10 秒封顶保证重启后很快连上。
 */
export const INITIAL_RECONNECT_DELAY_MS = 1_000;
export const MAX_RECONNECT_DELAY_MS = 10_000;

/**
 * 单次连接内遇到 cursor_expired 重新取快照订阅的最大次数：
 * 正常只需一次快照即可追平，上限 3 次防止事件保留窗口极小时在单连接上死循环。
 */
export const MAX_SNAPSHOT_RETRIES_PER_CONNECT = 3;

export function computeReconnectDelay(attempt: number): number {
  const safeAttempt = Math.max(0, Math.floor(attempt));
  return Math.min(
    INITIAL_RECONNECT_DELAY_MS * 2 ** safeAttempt,
    MAX_RECONNECT_DELAY_MS,
  );
}

export type ConnectionStatus =
  | { readonly kind: "connecting" }
  | { readonly kind: "connected"; readonly version: string }
  | {
      readonly kind: "reconnecting";
      readonly attempt: number;
      readonly delayMs: number;
    }
  | {
      readonly kind: "auth_expired";
      readonly cause: "login_link_invalid" | "session_expired";
    }
  | { readonly kind: "error"; readonly cause: "unsupported_version" };

export interface SocketLike {
  readonly readyState: number;
  onopen: ((event: unknown) => void) | null;
  onmessage: ((event: { data: unknown }) => void) | null;
  onerror: ((event: unknown) => void) | null;
  onclose: ((event: unknown) => void) | null;
  send(data: string): void;
  close(code?: number, reason?: string): void;
}

export type ServerProbeResult = "alive" | "unreachable";

export interface LoginExchangeContext {
  readonly hash: string;
  readonly pathname: string;
  readonly search: string;
  replaceUrl(url: string): void;
  fetchFn: typeof fetch;
}

export interface QoConnectionOptions {
  readonly url?: string;
  readonly getAppliedCursor: () => Decimal | null;
  readonly onSnapshot: (snapshot: StateGetResult) => void;
  readonly onEvent: (event: Event) => void;
  readonly onApprovalRequested: (request: ApprovalRequest) => void;
  readonly onInputRequested: (request: InputRequested) => void;
  readonly onStatusChange?: (status: ConnectionStatus) => void;
  readonly createSocket?: (url: string) => SocketLike;
  readonly probeServer?: () => Promise<ServerProbeResult>;
  readonly exchangeLogin?: () => Promise<"none" | "exchanged" | "invalid">;
  readonly scheduleTimer?: (fn: () => void, ms: number) => unknown;
  readonly cancelTimer?: (handle: unknown) => void;
}

/**
 * 所有带客户端幂等 ID 的写操作：请求在途断线时，重连后用同一个 ID 与相同参数重发一次。
 */
export const IDEMPOTENT_WRITE_METHODS: ReadonlySet<ProtocolMethodName> =
  new Set([
    "task.create",
    "session.open",
    "turn.start",
    "turn.append",
    "approval.decide",
  ]);

interface PendingRequest {
  method: ProtocolMethodName;
  params: unknown;
  idempotent: boolean;
  resolve: (value: unknown) => void;
  reject: (reason: unknown) => void;
}

interface QueuedRequest {
  method: ProtocolMethodName;
  params: unknown;
  resolve: (value: unknown) => void;
  reject: (reason: unknown) => void;
}

const SOCKET_OPEN = 1;

let browserLoginExchangePromise: Promise<
  "none" | "exchanged" | "invalid"
> | null = null;
let lastConsumedLoginCode: string | null = null;

export function readLoginCodeFromHash(hash: string): string | null {
  return new URLSearchParams(hash.replace(/^#/, "")).get("login");
}

export function isLoginCodeConsumed(code: string): boolean {
  return lastConsumedLoginCode === code;
}

export async function exchangeLoginCode(
  ctx?: LoginExchangeContext,
): Promise<"none" | "exchanged" | "invalid"> {
  const target: LoginExchangeContext | null =
    ctx ??
    (typeof location !== "undefined" && typeof history !== "undefined"
      ? {
          hash: location.hash,
          pathname: location.pathname,
          search: location.search,
          replaceUrl: (url: string) => history.replaceState(null, "", url),
          fetchFn: (input, init) => globalThis.fetch(input, init),
        }
      : null);

  if (!target) return "none";

  const code = readLoginCodeFromHash(target.hash);
  if (!code) return "none";

  target.replaceUrl(`${target.pathname}${target.search}`);
  const fetchFn = target.fetchFn;
  const response = await fetchFn("/auth/exchange", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ code }),
    credentials: "same-origin",
  });
  return response.ok ? "exchanged" : "invalid";
}

function exchangeBrowserLoginOnce(): Promise<"none" | "exchanged" | "invalid"> {
  if (!browserLoginExchangePromise) {
    if (typeof location !== "undefined") {
      const code = readLoginCodeFromHash(location.hash);
      if (code && code === lastConsumedLoginCode) {
        if (typeof history !== "undefined") {
          history.replaceState(
            null,
            "",
            `${location.pathname}${location.search}`,
          );
        }
        return Promise.resolve("none");
      }
      if (code) {
        lastConsumedLoginCode = code;
      }
    }
    browserLoginExchangePromise = exchangeLoginCode().catch((error) => {
      browserLoginExchangePromise = null;
      throw error;
    });
  }
  return browserLoginExchangePromise;
}

export function resetBrowserLoginExchange(): void {
  browserLoginExchangePromise = null;
}

/**
 * 浏览器 WebSocket API 不会向脚本暴露升级失败的 HTTP 状态码。
 * 若握手在 open 前失败，但同源根路径仍返回 2xx 或 403，说明本机服务在线，
 * /rpc 拒绝升级即为 403（会话 cookie 失效或未登录），此时停止自动重连。
 */
export async function probeSameOriginServer(
  fetchFn?: typeof fetch,
): Promise<ServerProbeResult> {
  try {
    const doFetch = fetchFn ?? ((input, init) => globalThis.fetch(input, init));
    const response = await doFetch("/", {
      method: "GET",
      cache: "no-store",
      credentials: "same-origin",
    });
    if (response.ok || response.status === 403) {
      return "alive";
    }
    return "unreachable";
  } catch {
    return "unreachable";
  }
}

function defaultWsUrl(): string {
  const protocol = location.protocol === "https:" ? "wss:" : "ws:";
  return `${protocol}//${location.host}/rpc`;
}

function parseRpcError(value: unknown): QoRpcError | null {
  if (!value || typeof value !== "object") return null;
  const obj = value as Record<string, unknown>;
  if (typeof obj.code !== "number" || typeof obj.message !== "string") {
    return null;
  }
  const rawData =
    obj.data && typeof obj.data === "object"
      ? (obj.data as Record<string, unknown>)
      : {};
  const data: RpcErrorData = {
    reason: typeof rawData.reason === "string" ? rawData.reason : "unknown",
    ...(typeof rawData.oldestCursor === "string"
      ? { oldestCursor: rawData.oldestCursor }
      : {}),
    ...(typeof rawData.highWater === "string"
      ? { highWater: rawData.highWater }
      : {}),
  };
  return new QoRpcError({
    code: obj.code,
    message: obj.message,
    data,
  });
}

export class QoConnection {
  private readonly options: QoConnectionOptions;
  private readonly wsUrl: string;
  private readonly createSocket: (url: string) => SocketLike;
  private readonly probeServer: () => Promise<ServerProbeResult>;
  private readonly exchangeLogin: () => Promise<
    "none" | "exchanged" | "invalid"
  >;
  private readonly scheduleTimer: (fn: () => void, ms: number) => unknown;
  private readonly cancelTimer: (handle: unknown) => void;

  private socket: SocketLike | null = null;
  private nextId = 1;
  private readonly pending = new Map<number, PendingRequest>();
  private readonly retryQueue: QueuedRequest[] = [];
  private reconnectTimer: unknown = null;
  private reconnectAttempt = 0;
  private disposed = false;
  private stopped = false;
  private connectionGeneration = 0;
  private status: ConnectionStatus = { kind: "connecting" };

  constructor(options: QoConnectionOptions) {
    this.options = options;
    this.wsUrl = options.url ?? defaultWsUrl();
    this.createSocket =
      options.createSocket ??
      ((url: string) => new WebSocket(url) as unknown as SocketLike);
    this.probeServer = options.probeServer ?? (() => probeSameOriginServer());
    this.exchangeLogin =
      options.exchangeLogin ?? (() => exchangeBrowserLoginOnce());
    this.scheduleTimer =
      options.scheduleTimer ?? ((fn, ms) => globalThis.setTimeout(fn, ms));
    this.cancelTimer =
      options.cancelTimer ??
      ((handle) =>
        globalThis.clearTimeout(handle as ReturnType<typeof setTimeout>));
  }

  getStatus(): ConnectionStatus {
    return this.status;
  }

  start(): void {
    if (this.disposed) return;
    this.stopped = false;
    this.setStatus({ kind: "connecting" });
    void this.bootstrap();
  }

  dispose(): void {
    this.disposed = true;
    this.stopped = true;
    this.connectionGeneration += 1;
    this.clearReconnectTimer();
    const current = this.socket;
    this.socket = null;
    if (current) {
      current.onopen = null;
      current.onmessage = null;
      current.onerror = null;
      current.onclose = null;
      current.close();
    }
    this.rejectAllRequests(new Error("连接已关闭"));
  }

  call<M extends ProtocolMethodName>(
    method: M,
    params: ProtocolMethods[M]["params"],
  ): Promise<ProtocolMethods[M]["result"]> {
    if (this.disposed || this.stopped) {
      return Promise.reject(new Error("连接已关闭"));
    }
    const idempotent = IDEMPOTENT_WRITE_METHODS.has(method);
    if (idempotent) {
      return new Promise((resolve, reject) => {
        const entry: QueuedRequest = {
          method,
          params,
          resolve: resolve as (value: unknown) => void,
          reject,
        };
        if (this.isReady()) {
          this.sendOnSocket(entry, true);
        } else {
          this.retryQueue.push(entry);
        }
      });
    }
    return this.sendDirect(method, params);
  }

  private isReady(): boolean {
    return (
      this.socket !== null &&
      this.socket.readyState === SOCKET_OPEN &&
      this.status.kind === "connected"
    );
  }

  private sendDirect<M extends ProtocolMethodName>(
    method: M,
    params: ProtocolMethods[M]["params"],
  ): Promise<ProtocolMethods[M]["result"]> {
    const socket = this.socket;
    if (!socket || socket.readyState !== SOCKET_OPEN) {
      return Promise.reject(new Error("WebSocket 尚未就绪"));
    }
    return new Promise((resolve, reject) => {
      this.sendOnSocket(
        {
          method,
          params,
          resolve: resolve as (value: unknown) => void,
          reject,
        },
        false,
      );
    });
  }

  private sendOnSocket(entry: QueuedRequest, idempotent: boolean): void {
    const socket = this.socket;
    if (!socket || socket.readyState !== SOCKET_OPEN) {
      if (idempotent && !this.disposed && !this.stopped) {
        this.retryQueue.push(entry);
      } else {
        entry.reject(new Error("WebSocket 尚未就绪"));
      }
      return;
    }
    const id = this.nextId;
    this.nextId += 1;
    this.pending.set(id, { ...entry, idempotent });
    try {
      socket.send(
        JSON.stringify({
          jsonrpc: "2.0",
          id,
          method: entry.method,
          params: entry.params,
        }),
      );
    } catch (error) {
      this.pending.delete(id);
      if (idempotent && !this.disposed && !this.stopped) {
        this.retryQueue.push(entry);
      } else {
        entry.reject(error);
      }
    }
  }

  private async bootstrap(): Promise<void> {
    try {
      const loginResult = await this.exchangeLogin();
      if (this.disposed) return;
      if (loginResult === "invalid") {
        this.stopped = true;
        this.rejectAllRequests(new Error("登录链接已失效"));
        this.setStatus({
          kind: "auth_expired",
          cause: "login_link_invalid",
        });
        return;
      }
      this.openSocket();
    } catch {
      if (this.disposed) return;
      this.scheduleReconnect();
    }
  }

  private openSocket(): void {
    if (this.disposed || this.stopped) return;
    this.clearReconnectTimer();
    this.connectionGeneration += 1;
    const generation = this.connectionGeneration;

    let socket: SocketLike;
    try {
      socket = this.createSocket(this.wsUrl);
    } catch {
      this.scheduleReconnect();
      return;
    }

    this.socket = socket;
    let didOpen = false;
    let closeHandled = false;

    const handleClosed = (closeEvent: unknown): void => {
      if (closeHandled || generation !== this.connectionGeneration) return;
      closeHandled = true;
      if (this.socket === socket) {
        this.socket = null;
      }
      this.handleDisconnectPending();
      if (this.disposed || this.stopped) {
        this.rejectAllRequests(new Error("连接已关闭"));
        return;
      }

      if (!didOpen) {
        const statusFromEvent = extractHttpStatus(closeEvent);
        if (statusFromEvent === 403) {
          this.stopped = true;
          this.rejectAllRequests(new Error("登录状态已失效"));
          this.setStatus({
            kind: "auth_expired",
            cause: "session_expired",
          });
          return;
        }
        void this.handlePreOpenFailure(generation);
        return;
      }

      this.scheduleReconnect();
    };

    socket.onopen = () => {
      if (this.disposed || generation !== this.connectionGeneration) {
        socket.close();
        return;
      }
      didOpen = true;
      void this.handshakeAndSync(generation, socket);
    };

    socket.onmessage = (event) => {
      if (this.disposed || generation !== this.connectionGeneration) return;
      this.handleMessage(event.data);
    };

    socket.onerror = () => {
      // 浏览器在握手失败或断线时会随后触发 onclose，统一在 onclose 处理。
    };

    socket.onclose = (event) => {
      handleClosed(event);
    };
  }

  private async handlePreOpenFailure(generation: number): Promise<void> {
    const probe = await this.probeServer();
    if (
      this.disposed ||
      this.stopped ||
      generation !== this.connectionGeneration
    ) {
      return;
    }
    if (probe === "alive") {
      this.stopped = true;
      this.rejectAllRequests(new Error("登录状态已失效"));
      this.setStatus({
        kind: "auth_expired",
        cause: "session_expired",
      });
      return;
    }
    this.scheduleReconnect();
  }

  private async handshakeAndSync(
    generation: number,
    socket: SocketLike,
  ): Promise<void> {
    try {
      const init = await this.sendDirect("initialize", { versions: ["qo.v1"] });
      if (this.disposed || generation !== this.connectionGeneration) return;
      if (init.version !== "qo.v1") {
        this.stopped = true;
        this.rejectAllRequests(new Error("协议版本不匹配"));
        this.setStatus({ kind: "error", cause: "unsupported_version" });
        socket.close();
        return;
      }

      const appliedCursor = this.options.getAppliedCursor();
      if (appliedCursor === null) {
        await this.snapshotAndSubscribe(generation);
      } else {
        try {
          await this.sendDirect("events.subscribe", {
            afterCursor: appliedCursor,
          });
        } catch (error) {
          if (isQoRpcError(error) && error.reason === "cursor_expired") {
            await this.snapshotAndSubscribe(generation);
          } else {
            throw error;
          }
        }
      }

      if (this.disposed || generation !== this.connectionGeneration) return;
      this.reconnectAttempt = 0;
      this.setStatus({ kind: "connected", version: init.version });
      this.flushRetryQueue();
    } catch (error) {
      if (this.disposed || generation !== this.connectionGeneration) return;
      if (isQoRpcError(error) && error.reason === "unsupported_version") {
        this.stopped = true;
        this.rejectAllRequests(new Error("协议版本不匹配"));
        this.setStatus({ kind: "error", cause: "unsupported_version" });
        socket.close();
        return;
      }
      // 握手或同步失败时关闭当前连接，交给 onclose 触发退避重连。
      socket.close();
    }
  }

  private async snapshotAndSubscribe(generation: number): Promise<void> {
    for (
      let attempt = 0;
      attempt < MAX_SNAPSHOT_RETRIES_PER_CONNECT;
      attempt += 1
    ) {
      const snapshot = await this.sendDirect("state.get", {});
      if (this.disposed || generation !== this.connectionGeneration) return;
      this.options.onSnapshot(snapshot);
      try {
        await this.sendDirect("events.subscribe", {
          afterCursor: snapshot.cursor,
        });
        return;
      } catch (error) {
        const isLast = attempt === MAX_SNAPSHOT_RETRIES_PER_CONNECT - 1;
        if (
          !isLast &&
          isQoRpcError(error) &&
          error.reason === "cursor_expired"
        ) {
          continue;
        }
        throw error;
      }
    }
  }

  private handleMessage(raw: unknown): void {
    if (typeof raw !== "string") return;
    let parsed: unknown;
    try {
      parsed = JSON.parse(raw);
    } catch {
      return;
    }
    if (!parsed || typeof parsed !== "object") return;
    const message = parsed as Record<string, unknown>;

    if ("id" in message && typeof message.id === "number") {
      const entry = this.pending.get(message.id);
      if (!entry) return;
      this.pending.delete(message.id);
      if ("error" in message && message.error !== undefined) {
        const rpcError =
          parseRpcError(message.error) ?? new Error("RPC 请求失败");
        entry.reject(rpcError);
      } else {
        entry.resolve(message.result);
      }
      return;
    }

    if (typeof message.method !== "string" || !("params" in message)) return;
    switch (message.method) {
      case "event":
        this.options.onEvent(message.params as Event);
        break;
      case "approval.requested":
        this.options.onApprovalRequested(message.params as ApprovalRequest);
        break;
      case "input.requested":
        this.options.onInputRequested(message.params as InputRequested);
        break;
      default:
        break;
    }
  }

  private scheduleReconnect(): void {
    if (this.disposed || this.stopped) return;
    this.clearReconnectTimer();
    const attempt = this.reconnectAttempt;
    const delayMs = computeReconnectDelay(attempt);
    this.reconnectAttempt += 1;
    this.setStatus({
      kind: "reconnecting",
      attempt: attempt + 1,
      delayMs,
    });
    this.reconnectTimer = this.scheduleTimer(() => {
      this.reconnectTimer = null;
      this.openSocket();
    }, delayMs);
  }

  private clearReconnectTimer(): void {
    if (this.reconnectTimer !== null) {
      this.cancelTimer(this.reconnectTimer);
      this.reconnectTimer = null;
    }
  }

  private handleDisconnectPending(): void {
    if (this.pending.size === 0) return;
    const entries = [...this.pending.values()];
    this.pending.clear();
    const toRequeue: QueuedRequest[] = [];
    const disconnectError = new Error("连接已断开");
    for (const entry of entries) {
      if (entry.idempotent && !this.disposed && !this.stopped) {
        toRequeue.push({
          method: entry.method,
          params: entry.params,
          resolve: entry.resolve,
          reject: entry.reject,
        });
      } else {
        entry.reject(disconnectError);
      }
    }
    if (toRequeue.length > 0) {
      this.retryQueue.unshift(...toRequeue);
    }
  }

  private flushRetryQueue(): void {
    if (this.retryQueue.length === 0 || !this.isReady()) return;
    const batch = this.retryQueue.splice(0, this.retryQueue.length);
    for (const entry of batch) {
      this.sendOnSocket(entry, true);
    }
  }

  private rejectAllRequests(reason: Error): void {
    if (this.pending.size > 0) {
      const entries = [...this.pending.values()];
      this.pending.clear();
      for (const entry of entries) {
        entry.reject(reason);
      }
    }
    if (this.retryQueue.length > 0) {
      const queued = this.retryQueue.splice(0, this.retryQueue.length);
      for (const entry of queued) {
        entry.reject(reason);
      }
    }
  }

  private setStatus(status: ConnectionStatus): void {
    this.status = status;
    this.options.onStatusChange?.(status);
  }
}

function extractHttpStatus(event: unknown): number | null {
  if (!event || typeof event !== "object") return null;
  const obj = event as Record<string, unknown>;
  return typeof obj.httpStatus === "number" ? obj.httpStatus : null;
}
