import {
  useEffect,
  useMemo,
  useRef,
  useState,
  useSyncExternalStore,
} from "react";
import type {
  Id,
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
import {
  getNeedsAttentionItems,
  getTaskSessions,
  groupVisibleTasks,
  type TaskFilters,
} from "./state.js";
import { QoStore } from "./store.js";

function connectionBanner(
  status: ConnectionStatus,
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
          Math.round(status.delayMs / 1000),
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

  const banner = connectionBanner(connectionStatus);
  const isConnected = connectionStatus.kind === "connected";

  return (
    <div className="workbench">
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

      <main className="main-pane">
        {banner ? (
          <div
            className={
              banner.isAction
                ? "connection-banner connection-banner-action"
                : "connection-banner"
            }
            role="status"
            aria-live="polite"
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
