import { useEffect, useMemo, useState, useSyncExternalStore } from "react";
import type { Id } from "@qo/protocol";
import { Sidebar } from "./components/Sidebar.js";
import { TaskOverview } from "./components/TaskOverview.js";
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

  const [filters, setFilters] = useState<TaskFilters>(() => loadTaskFilters());
  const [selectedTaskId, setSelectedTaskId] = useState<Id | null>(null);
  const [selectedSessionId, setSelectedSessionId] = useState<Id | null>(null);

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
      connection.start();
    };
    window.addEventListener("hashchange", handleHashChange);

    return () => {
      window.removeEventListener("hashchange", handleHashChange);
      connection.dispose();
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

  const handleFiltersChange = (next: TaskFilters) => {
    setFilters(next);
    saveTaskFilters(next);
  };

  const handleSelectTask = (taskId: Id, sessionId?: Id) => {
    setSelectedTaskId(taskId);
    setSelectedSessionId(sessionId ?? null);
  };

  const banner = connectionBanner(connectionStatus);

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
          <TaskOverview
            state={state}
            connectionStatus={connectionStatus}
            selectedTask={selectedTask}
            selectedSessionId={effectiveSessionId}
            onSelectSession={setSelectedSessionId}
          />
        </div>
      </main>
    </div>
  );
}
