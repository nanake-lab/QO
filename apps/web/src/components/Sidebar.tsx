import type { Id, Session, Task } from "@qo/protocol";
import type { ConnectionStatus } from "../connection.js";
import { zhCN } from "../i18n/zh-CN.js";
import type { NeedsAttentionItem, TaskFilters, TaskGroup } from "../state.js";

interface SidebarProps {
  readonly connectionStatus: ConnectionStatus;
  readonly cursor: string | null;
  readonly attentionItems: readonly NeedsAttentionItem[];
  readonly taskGroups: readonly TaskGroup[];
  readonly totalTaskCount: number;
  readonly filters: TaskFilters;
  readonly selectedTaskId: Id | null;
  readonly selectedSessionId: Id | null;
  readonly onFiltersChange: (next: TaskFilters) => void;
  readonly onSelectTask: (taskId: Id, sessionId?: Id) => void;
}

function renderTaskStatus(
  task: Task,
  urgentSession: Session | null,
): { label: string; className: string } {
  if (!urgentSession) {
    return {
      label: zhCN.sidebar.noSessions,
      className: "status-text status-text-muted",
    };
  }
  if (
    task.createdBy === "external" &&
    (urgentSession.state === "waiting_approval" ||
      urgentSession.state === "waiting_input")
  ) {
    return {
      label: zhCN.sidebar.externalWaiting,
      className: "status-text status-text-muted",
    };
  }
  if (
    urgentSession.state === "waiting_approval" ||
    urgentSession.state === "waiting_input"
  ) {
    return {
      label: zhCN.sessionState[urgentSession.state],
      className: "status-text status-text-action",
    };
  }
  if (urgentSession.state === "running" || urgentSession.state === "starting") {
    return {
      label: zhCN.sessionState[urgentSession.state],
      className: "status-text status-text-active",
    };
  }
  if (urgentSession.state === "failed") {
    return {
      label: zhCN.sessionState.failed,
      className: "status-text status-text-failed",
    };
  }
  return {
    label: zhCN.sessionState[urgentSession.state],
    className: "status-text status-text-muted",
  };
}

function connectionPill(status: ConnectionStatus): {
  label: string;
  dotClass: string;
} {
  switch (status.kind) {
    case "connected":
      return {
        label: zhCN.connection.connected(status.version),
        dotClass: "conn-dot conn-dot-connected",
      };
    case "connecting":
      return {
        label: zhCN.connection.connecting,
        dotClass: "conn-dot",
      };
    case "reconnecting":
      return {
        label: zhCN.connection.reconnectingShort,
        dotClass: "conn-dot",
      };
    case "auth_expired":
      return {
        label: zhCN.connection.authExpiredShort,
        dotClass: "conn-dot conn-dot-action",
      };
    case "error":
      return {
        label: zhCN.connection.errorShort,
        dotClass: "conn-dot",
      };
  }
}

export function Sidebar({
  connectionStatus,
  cursor,
  attentionItems,
  taskGroups,
  totalTaskCount,
  filters,
  selectedTaskId,
  selectedSessionId,
  onFiltersChange,
  onSelectTask,
}: SidebarProps) {
  const pill = connectionPill(connectionStatus);
  const hasAttention = attentionItems.length > 0;

  return (
    <aside className="sidebar" aria-label={zhCN.sidebar.navLabel}>
      <header className="sidebar-header">
        <h1 className="brand">{zhCN.app.name}</h1>
        <span className="conn-pill" role="status" aria-live="polite">
          <span className={pill.dotClass} aria-hidden="true" />
          <span>{pill.label}</span>
        </span>
      </header>

      <section
        className="attention-section"
        aria-label={zhCN.sidebar.attentionHeading}
      >
        <div className="section-heading-row">
          <h2
            className={
              hasAttention
                ? "section-heading section-heading-action"
                : "section-heading"
            }
          >
            {zhCN.sidebar.attentionHeading}
          </h2>
          <span
            className={
              hasAttention
                ? "count-badge count-badge-action tabular"
                : "count-badge tabular"
            }
          >
            {attentionItems.length}
          </span>
        </div>

        {hasAttention ? (
          <ul className="attention-list">
            {attentionItems.map(({ task, session, approval, input }) => {
              const selected =
                selectedTaskId === task.taskId &&
                selectedSessionId === session.sessionId;
              const preview =
                approval?.summary ?? input?.questions[0]?.question ?? null;
              return (
                <li key={session.sessionId}>
                  <button
                    type="button"
                    className={
                      selected
                        ? "attention-item attention-item-selected"
                        : "attention-item"
                    }
                    aria-current={selected ? "true" : undefined}
                    onClick={() => onSelectTask(task.taskId, session.sessionId)}
                  >
                    <span className="attention-item-top">
                      <span className="attention-task-name">{task.name}</span>
                      <span className="attention-status">
                        {zhCN.sessionState[session.state]}
                      </span>
                    </span>
                    {preview ? (
                      <span className="attention-summary">{preview}</span>
                    ) : null}
                  </button>
                </li>
              );
            })}
          </ul>
        ) : (
          <p className="attention-empty">{zhCN.sidebar.attentionEmpty}</p>
        )}
      </section>

      <div
        className="filters-bar"
        role="group"
        aria-label={zhCN.sidebar.filtersLabel}
      >
        <label className="filter-checkbox">
          <input
            type="checkbox"
            checked={filters.hideClosed}
            onChange={(event) =>
              onFiltersChange({
                ...filters,
                hideClosed: event.currentTarget.checked,
              })
            }
          />
          <span>{zhCN.sidebar.hideClosed}</span>
        </label>
        <label className="filter-checkbox">
          <input
            type="checkbox"
            checked={filters.onlyMine}
            onChange={(event) =>
              onFiltersChange({
                ...filters,
                onlyMine: event.currentTarget.checked,
              })
            }
          />
          <span>{zhCN.sidebar.onlyMine}</span>
        </label>
      </div>

      <nav className="task-list-scroll" aria-label={zhCN.sidebar.tasksHeading}>
        {taskGroups.length === 0 ? (
          <p className="task-empty">
            {totalTaskCount === 0
              ? zhCN.sidebar.emptyTasks
              : zhCN.sidebar.emptyFilteredTasks}
          </p>
        ) : (
          taskGroups.map((group) => (
            <section
              key={group.group ?? "__ungrouped__"}
              className="task-group"
            >
              <h3 className="task-group-title">
                {group.group ?? zhCN.sidebar.ungrouped}
              </h3>
              <ul className="task-group-list">
                {group.tasks.map(({ task, urgentSession }) => {
                  const selected = selectedTaskId === task.taskId;
                  const status = renderTaskStatus(task, urgentSession);
                  const rowClasses = [
                    "task-row",
                    selected ? "task-row-selected" : "",
                    task.state === "closed" ? "task-row-closed" : "",
                  ]
                    .filter(Boolean)
                    .join(" ");
                  return (
                    <li key={task.taskId}>
                      <button
                        type="button"
                        className={rowClasses}
                        aria-current={selected ? "true" : undefined}
                        onClick={() =>
                          onSelectTask(task.taskId, urgentSession?.sessionId)
                        }
                      >
                        <span className="task-row-top">
                          <span className="task-row-name">{task.name}</span>
                          <span className="creator-tag">
                            {zhCN.creator[task.createdBy]}
                          </span>
                        </span>
                        <span className="task-row-meta">
                          <span className={status.className}>
                            {status.label}
                          </span>
                          {task.state === "closed" ? (
                            <span className="task-closed-tag">
                              {zhCN.taskState.closed}
                            </span>
                          ) : null}
                        </span>
                      </button>
                    </li>
                  );
                })}
              </ul>
            </section>
          ))
        )}
      </nav>

      <footer className="sidebar-footer tabular">
        <span>{zhCN.sidebar.tasksHeading}</span>
        <span>
          {cursor !== null ? zhCN.connection.cursorLabel(cursor) : "—"}
        </span>
      </footer>
    </aside>
  );
}
