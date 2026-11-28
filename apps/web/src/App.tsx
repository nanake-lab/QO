import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  useSyncExternalStore,
} from "react";
import type {
  ApprovalDecideParams,
  ApprovalRequest,
  Id,
  InputAnswerParams,
  InputRequested,
  Session,
  SessionOpenParams,
  TaskCreateParams,
  TurnAppendParams,
  TurnInterruptParams,
  TurnStartParams,
} from "@qo/protocol";
import { CreateTaskDialog } from "./components/CreateTaskDialog.js";
import { Sidebar } from "./components/Sidebar.js";
import { TaskWorkspace } from "./components/TaskWorkspace.js";
import {
  QoConnection,
  isLoginCodeConsumed,
  readLoginCodeFromHash,
  resetBrowserLoginExchange,
  type ConnectionStatus,
} from "./connection.js";
import { loadTaskFilters, saveTaskFilters } from "./filters.js";
import { zhCN } from "./i18n/zh-CN.js";
import { isQoRpcError } from "./protocol.js";
import {
  getNeedsAttentionItems,
  getTaskSessions,
  groupVisibleTasks,
  type TaskFilters,
} from "./state.js";
import { QoStore } from "./store.js";

/** 重连倒计时刷新周期（1 秒）：顶部提示按秒更新重试倒计时。 */
const RECONNECT_COUNTDOWN_TICK_MS = 1_000;

function connectionBanner(
  status: ConnectionStatus,
  reconnectRemainingSec: number | null,
): { text: string; isAction: boolean } | null {
  switch (status.kind) {
    case "connected":
      return null;
    case "connecting":
      return { text: zhCN.connection.connecting, isAction: false };
    case "reconnecting":
      return {
        text: zhCN.connection.reconnecting(
          status.attempt,
          reconnectRemainingSec ??
            Math.max(1, Math.ceil(status.delayMs / 1_000)),
        ),
        isAction: false,
      };
    case "auth_expired":
      return {
        text:
          status.cause === "login_link_invalid"
            ? zhCN.connection.loginLinkInvalidBanner
            : zhCN.connection.authExpiredBanner,
        isAction: true,
      };
    case "error":
      return {
        text: zhCN.connection.unsupportedVersionBanner,
        isAction: false,
      };
  }
}

export function App() {
  const [store] = useState(() => new QoStore());
  const snapshot = useSyncExternalStore(store.subscribe, store.getSnapshot);
  const { state, connectionStatus } = snapshot;

  const connectionRef = useRef<QoConnection | null>(null);
  const [filters, setFilters] = useState<TaskFilters>(() => loadTaskFilters());
  const [selectedTaskId, setSelectedTaskId] = useState<Id | null>(null);
  const [selectedSessionId, setSelectedSessionId] = useState<Id | null>(null);
  const [showCreateTask, setShowCreateTask] = useState(false);
  const [reconnectRemainingSec, setReconnectRemainingSec] = useState<
    number | null
  >(null);
  const [liveAnnouncement, setLiveAnnouncement] = useState("");
  const prevConnKeyRef = useRef<string | null>(null);
  const prevAttentionCountRef = useRef<number | null>(null);
  const prevSessionRef = useRef<{ sessionId: Id; state: string } | null>(null);

  useEffect(() => {
    const createConnection = () =>
      new QoConnection({
        getAppliedCursor: store.getAppliedCursor,
        onSnapshot: store.onSnapshot,
        onEvent: store.onEvent,
        onApprovalRequested: store.onApprovalRequested,
        onInputRequested: store.onInputRequested,
        onStatusChange: store.onStatusChange,
      });

    let connection = createConnection();
    connectionRef.current = connection;
    connection.start();

    const handleHashChange = () => {
      const code = readLoginCodeFromHash(location.hash);
      if (!code) return;
      if (isLoginCodeConsumed(code)) {
        history.replaceState(
          null,
          "",
          `${location.pathname}${location.search}`,
        );
        return;
      }
      resetBrowserLoginExchange();
      connection.dispose();
      connection = createConnection();
      connectionRef.current = connection;
      connection.start();
    };
    window.addEventListener("hashchange", handleHashChange);

    return () => {
      window.removeEventListener("hashchange", handleHashChange);
      connection.dispose();
      if (connectionRef.current === connection) {
        connectionRef.current = null;
      }
    };
  }, [store]);

  const attentionItems = useMemo(() => getNeedsAttentionItems(state), [state]);
  const taskGroups = useMemo(
    () => groupVisibleTasks(state, filters),
    [state, filters],
  );
  const totalTaskCount = Object.keys(state.tasks).length;

  useEffect(() => {
    document.title = zhCN.app.documentTitle(attentionItems.length);
  }, [attentionItems.length]);

  const effectiveTaskId = useMemo(() => {
    if (selectedTaskId && state.tasks[selectedTaskId]) {
      return selectedTaskId;
    }
    const firstVisible = taskGroups[0]?.tasks[0]?.task.taskId ?? null;
    return firstVisible;
  }, [selectedTaskId, state.tasks, taskGroups]);

  const selectedTask = effectiveTaskId
    ? (state.tasks[effectiveTaskId] ?? null)
    : null;

  const effectiveSessionId = useMemo(() => {
    if (!selectedTask) return null;
    const sessions = getTaskSessions(state, selectedTask.taskId);
    if (
      selectedSessionId &&
      sessions.some((s) => s.sessionId === selectedSessionId)
    ) {
      return selectedSessionId;
    }
    return sessions[0]?.sessionId ?? null;
  }, [selectedTask, selectedSessionId, state]);

  const activeSession = effectiveSessionId
    ? (state.sessions[effectiveSessionId] ?? null)
    : null;

  useEffect(() => {
    if (connectionStatus.kind !== "reconnecting") {
      setReconnectRemainingSec(null);
      return;
    }
    const deadlineMs = Date.now() + connectionStatus.delayMs;
    const updateRemaining = () => {
      const remaining = Math.max(
        0,
        Math.ceil((deadlineMs - Date.now()) / 1_000),
      );
      setReconnectRemainingSec(remaining);
    };
    updateRemaining();
    const timer = window.setInterval(
      updateRemaining,
      RECONNECT_COUNTDOWN_TICK_MS,
    );
    return () => window.clearInterval(timer);
  }, [connectionStatus]);

  useEffect(() => {
    const key =
      connectionStatus.kind === "reconnecting"
        ? `reconnecting:${connectionStatus.attempt}`
        : connectionStatus.kind === "auth_expired"
          ? `auth_expired:${connectionStatus.cause}`
          : connectionStatus.kind;
    const prev = prevConnKeyRef.current;
    prevConnKeyRef.current = key;
    if (prev === null || prev === key) return;

    if (connectionStatus.kind === "connected") {
      setLiveAnnouncement(zhCN.app.liveConnected);
    } else if (connectionStatus.kind === "reconnecting") {
      setLiveAnnouncement(zhCN.app.liveDisconnected(connectionStatus.attempt));
    } else if (connectionStatus.kind === "auth_expired") {
      setLiveAnnouncement(
        connectionStatus.cause === "login_link_invalid"
          ? zhCN.connection.loginLinkInvalidBanner
          : zhCN.connection.authExpiredBanner,
      );
    } else if (connectionStatus.kind === "error") {
      setLiveAnnouncement(zhCN.connection.unsupportedVersionBanner);
    }
  }, [connectionStatus]);

  useEffect(() => {
    if (state.cursor === null) return;
    const count = attentionItems.length;
    const prev = prevAttentionCountRef.current;
    prevAttentionCountRef.current = count;
    if (prev === null || prev === count) return;
    setLiveAnnouncement(zhCN.app.liveAttentionCount(count));
  }, [state.cursor, attentionItems.length]);

  useEffect(() => {
    if (!selectedTask || !activeSession) {
      prevSessionRef.current = null;
      return;
    }
    const prev = prevSessionRef.current;
    prevSessionRef.current = {
      sessionId: activeSession.sessionId,
      state: activeSession.state,
    };
    if (
      !prev ||
      prev.sessionId !== activeSession.sessionId ||
      prev.state === activeSession.state
    ) {
      return;
    }
    const stateText =
      selectedTask.createdBy === "external" &&
      (activeSession.state === "waiting_approval" ||
        activeSession.state === "waiting_input")
        ? zhCN.overview.waitingExternalApproval
        : zhCN.sessionState[activeSession.state];
    setLiveAnnouncement(
      zhCN.app.liveSessionState(selectedTask.name, stateText),
    );
  }, [selectedTask, activeSession]);

  const requireConnection = (): QoConnection => {
    const conn = connectionRef.current;
    if (!conn) {
      throw new Error("连接尚未初始化");
    }
    return conn;
  };

  const handleFiltersChange = (next: TaskFilters) => {
    setFilters(next);
    saveTaskFilters(next);
  };

  const handleSelectTask = (taskId: Id, sessionId?: Id) => {
    setSelectedTaskId(taskId);
    setSelectedSessionId(sessionId ?? null);
  };

  const handleCreateTask = async (params: TaskCreateParams) => {
    const created = await requireConnection().call("task.create", params);
    setSelectedTaskId(created.taskId);
    setSelectedSessionId(null);
  };

  const handleOpenSession = async (
    params: SessionOpenParams,
  ): Promise<Session> => {
    return requireConnection().call("session.open", params);
  };

  const handleFinishSession = async (taskId: Id, sessionId: Id) => {
    await requireConnection().call("session.finish", { taskId, sessionId });
  };

  const handleCancelSession = async (taskId: Id, sessionId: Id) => {
    await requireConnection().call("session.cancel", { taskId, sessionId });
  };

  const handleCloseTask = async (taskId: Id) => {
    await requireConnection().call("task.close", { taskId });
  };

  const handleStartTurn = async (params: TurnStartParams) => {
    await requireConnection().call("turn.start", params);
  };

  const handleAppendTurn = async (params: TurnAppendParams) => {
    await requireConnection().call("turn.append", params);
  };

  const handleInterruptTurn = async (params: TurnInterruptParams) => {
    await requireConnection().call("turn.interrupt", params);
  };

  const handleRefreshApprovals = useCallback(
    async (taskId: Id) => {
      const result = await requireConnection().call("approval.list", {
        taskId,
      });
      store.syncTaskApprovals(taskId, result.requests);
    },
    [store],
  );

  const handleDecideApproval = async (
    request: ApprovalRequest,
    params: ApprovalDecideParams,
  ) => {
    store.setApprovalDeciding(request.approvalId, true);
    try {
      const result = await requireConnection().call("approval.decide", params);
      store.recordApprovalDecision(request, result);
    } catch (error: unknown) {
      store.setApprovalDeciding(request.approvalId, false);
      if (isQoRpcError(error)) {
        if (error.reason === "approval_expired") {
          store.markApprovalExpired(request.approvalId);
        } else if (error.reason === "request_withdrawn") {
          store.markApprovalWithdrawn(request);
        }
      }
      throw error;
    }
  };

  const handleRenewApproval = async (request: ApprovalRequest) => {
    try {
      const renewed = await requireConnection().call("approval.renew", {
        taskId: request.taskId,
        approvalId: request.approvalId,
      });
      store.replaceRenewedApproval(request.approvalId, renewed);
    } catch (error: unknown) {
      if (isQoRpcError(error) && error.reason === "request_withdrawn") {
        store.markApprovalWithdrawn(request);
      }
      throw error;
    }
  };

  const handleAnswerInput = async (
    request: InputRequested,
    params: InputAnswerParams,
  ) => {
    store.setInputAnswering(request.requestId, true);
    try {
      await requireConnection().call("input.answer", params);
      store.removeInputRequest(request.requestId);
    } catch (error: unknown) {
      if (isQoRpcError(error) && error.reason === "request_withdrawn") {
        store.markInputWithdrawn(request);
      } else {
        store.setInputAnswering(request.requestId, false);
      }
      throw error;
    }
  };

  const banner = connectionBanner(connectionStatus, reconnectRemainingSec);
  const isConnected = connectionStatus.kind === "connected";

  return (
    <div className="workbench">
      <a href="#main-content" className="skip-link">
        {zhCN.app.skipToMain}
      </a>

      <div
        className="sr-only"
        role="status"
        aria-live="polite"
        aria-atomic="true"
      >
        {liveAnnouncement}
      </div>

      <Sidebar
        connectionStatus={connectionStatus}
        cursor={state.cursor}
        attentionItems={attentionItems}
        taskGroups={taskGroups}
        totalTaskCount={totalTaskCount}
        filters={filters}
        selectedTaskId={effectiveTaskId}
        selectedSessionId={effectiveSessionId}
        onFiltersChange={handleFiltersChange}
        onSelectTask={handleSelectTask}
        onOpenCreateTask={() => setShowCreateTask(true)}
      />

      <main id="main-content" className="main-pane" tabIndex={-1}>
        {banner ? (
          <div
            className={
              banner.isAction
                ? "connection-banner connection-banner-action tabular"
                : "connection-banner tabular"
            }
          >
            {banner.text}
          </div>
        ) : null}

        <div className="main-scroll">
          <TaskWorkspace
            state={state}
            connectionStatus={connectionStatus}
            selectedTask={selectedTask}
            selectedSessionId={effectiveSessionId}
            onSelectSession={setSelectedSessionId}
            onOpenCreateTask={() => setShowCreateTask(true)}
            onOpenSession={handleOpenSession}
            onFinishSession={handleFinishSession}
            onCancelSession={handleCancelSession}
            onCloseTask={handleCloseTask}
            onStartTurn={handleStartTurn}
            onAppendTurn={handleAppendTurn}
            onInterruptTurn={handleInterruptTurn}
            onDecideApproval={handleDecideApproval}
            onRenewApproval={handleRenewApproval}
            onRefreshApprovals={handleRefreshApprovals}
            onAnswerInput={handleAnswerInput}
          />
        </div>
      </main>

      {showCreateTask ? (
        <CreateTaskDialog
          isConnected={isConnected}
          onCreate={handleCreateTask}
          onClose={() => setShowCreateTask(false)}
        />
      ) : null}
    </div>
  );
}
