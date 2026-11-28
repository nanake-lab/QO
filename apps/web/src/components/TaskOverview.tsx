import type { Id, Session, Task } from "@qo/protocol";
import type { ConnectionStatus } from "../connection.js";
import { zhCN } from "../i18n/zh-CN.js";
import {
  getSessionTurns,
  getTaskSessions,
  getTurnItems,
  type AppState,
} from "../state.js";

interface TaskOverviewProps {
  readonly state: AppState;
  readonly connectionStatus: ConnectionStatus;
  readonly selectedTask: Task | null;
  readonly selectedSessionId: Id | null;
  readonly onSelectSession: (sessionId: Id) => void;
}

function sessionStatusText(task: Task, session: Session): string {
  if (
    task.createdBy === "external" &&
    (session.state === "waiting_approval" || session.state === "waiting_input")
  ) {
    return zhCN.overview.waitingExternalApproval;
  }
  return zhCN.sessionState[session.state];
}

export function TaskOverview({
  state,
  connectionStatus,
  selectedTask,
  selectedSessionId,
  onSelectSession,
}: TaskOverviewProps) {
  const totalTasks = Object.keys(state.tasks).length;

  if (!selectedTask) {
    let title: string = zhCN.overview.noSelectionTitle;
    let body: string = zhCN.overview.noSelectionBody;

    if (totalTasks === 0) {
      if (state.cursor === null) {
        if (connectionStatus.kind === "auth_expired") {
          title = zhCN.overview.authRequiredTitle;
          body = zhCN.overview.authRequiredBody;
        } else if (connectionStatus.kind === "error") {
          title = zhCN.overview.errorTitle;
          body = zhCN.overview.errorBody;
        } else {
          title = zhCN.overview.connectingTitle;
          body = zhCN.overview.connectingBody;
        }
      } else {
        title = zhCN.overview.noTasksTitle;
        body = zhCN.overview.noTasksBody;
      }
    }

    return (
      <section className="empty-state">
        <h2 className="empty-state-title">{title}</h2>
        <p className="empty-state-body">{body}</p>
      </section>
    );
  }

  const sessions = getTaskSessions(state, selectedTask.taskId);
  const activeSession =
    sessions.find((s) => s.sessionId === selectedSessionId) ??
    sessions[0] ??
    null;
  const turns = activeSession
    ? getSessionTurns(state, activeSession.sessionId)
    : [];

  return (
    <article className="task-overview">
      <header className="overview-header">
        <div className="overview-title-row">
          <h2 className="overview-title">{selectedTask.name}</h2>
          <span className="meta-pill">
            {zhCN.creator[selectedTask.createdBy]}
          </span>
          <span className="meta-pill">
            {zhCN.taskState[selectedTask.state]}
          </span>
          {selectedTask.group?.trim() ? (
            <span className="meta-pill">{selectedTask.group.trim()}</span>
          ) : null}
        </div>
        <div className="overview-paths">
          <div className="path-row">
            <span className="path-label">{zhCN.overview.cwdLabel}</span>
            <code className="mono">{selectedTask.cwd}</code>
          </div>
          {selectedTask.forbiddenRoots.length > 0 ? (
            <div className="path-row">
              <span className="path-label">
                {zhCN.overview.forbiddenRootsLabel}
              </span>
              <code className="mono">
                {selectedTask.forbiddenRoots.join(", ")}
              </code>
            </div>
          ) : null}
        </div>
      </header>

      {selectedTask.createdBy === "external" ? (
        <div className="notice-bar">{zhCN.overview.externalNotice}</div>
      ) : null}

      {sessions.length === 0 ? (
        <p className="empty-state-body">{zhCN.overview.noSessionsBody}</p>
      ) : (
        <>
          <div
            className="session-tabs"
            role="tablist"
            aria-label={zhCN.overview.sessionsHeading}
          >
            {sessions.map((session, index) => {
              const selected = activeSession?.sessionId === session.sessionId;
              const needsOwnAction =
                selectedTask.createdBy === "ui" &&
                (session.state === "waiting_approval" ||
                  session.state === "waiting_input");
              const classes = [
                "session-tab",
                selected ? "session-tab-selected" : "",
                needsOwnAction ? "session-tab-action" : "",
              ]
                .filter(Boolean)
                .join(" ");
              return (
                <button
                  key={session.sessionId}
                  type="button"
                  role="tab"
                  aria-selected={selected}
                  className={classes}
                  onClick={() => onSelectSession(session.sessionId)}
                >
                  <span className="tabular">
                    {zhCN.overview.sessionLabel(index + 1)}
                  </span>
                  <span
                    className={
                      needsOwnAction
                        ? "status-text-action"
                        : "status-text-muted"
                    }
                  >
                    {sessionStatusText(selectedTask, session)}
                  </span>
                </button>
              );
            })}
          </div>

          {activeSession ? (
            <>
              {selectedTask.createdBy === "ui" &&
              activeSession.state === "waiting_approval" ? (
                <div className="notice-bar notice-bar-action" role="status">
                  {zhCN.overview.waitingYourApproval}
                </div>
              ) : null}

              {selectedTask.createdBy === "ui" &&
              activeSession.state === "waiting_input" ? (
                <div className="notice-bar notice-bar-action" role="status">
                  {zhCN.overview.waitingYourInput}
                </div>
              ) : null}

              {selectedTask.createdBy === "external" &&
              (activeSession.state === "waiting_approval" ||
                activeSession.state === "waiting_input") ? (
                <div className="notice-bar" role="status">
                  {zhCN.overview.waitingExternalApproval}
                </div>
              ) : null}

              <section className="session-card">
                <div className="session-meta-grid">
                  <div className="meta-field">
                    <span className="meta-field-label">
                      {zhCN.overview.harnessLabel}
                    </span>
                    <span className="meta-field-value mono tabular">
                      {activeSession.harness} {activeSession.harnessVersion}
                    </span>
                  </div>
                  <div className="meta-field">
                    <span className="meta-field-label">
                      {zhCN.overview.modelLabel}
                    </span>
                    <span className="meta-field-value mono">
                      {activeSession.model ?? zhCN.overview.unsetValue}
                    </span>
                  </div>
                  <div className="meta-field">
                    <span className="meta-field-label">
                      {zhCN.overview.reasoningEffortLabel}
                    </span>
                    <span className="meta-field-value">
                      {activeSession.reasoningEffort ??
                        zhCN.overview.unsetValue}
                    </span>
                  </div>
                  <div className="meta-field">
                    <span className="meta-field-label">
                      {zhCN.overview.serviceTierLabel}
                    </span>
                    <span className="meta-field-value">
                      {activeSession.serviceTier ?? zhCN.overview.unsetValue}
                    </span>
                  </div>
                </div>

                {activeSession.cancelledBy ? (
                  <div className="meta-field">
                    <span className="meta-field-value">
                      {zhCN.cancelledBy[activeSession.cancelledBy]}
                    </span>
                  </div>
                ) : null}

                {activeSession.error ? (
                  <div className="meta-field">
                    <span className="meta-field-label">
                      {zhCN.overview.sessionErrorLabel}
                    </span>
                    <span className="meta-field-value status-text-failed">
                      {activeSession.error}
                    </span>
                  </div>
                ) : null}
              </section>

              <section className="turns-section">
                <h3 className="turns-heading">{zhCN.overview.turnsHeading}</h3>
                {turns.length === 0 ? (
                  <p className="empty-state-body">
                    {zhCN.overview.noTurnsBody}
                  </p>
                ) : (
                  turns.map((turn) => {
                    const items = getTurnItems(state, turn.turnId);
                    return (
                      <div key={turn.turnId} className="turn-card">
                        <div className="turn-input-block">
                          <span className="block-label">
                            {zhCN.overview.userInputLabel}
                          </span>
                          <p className="turn-input-text">{turn.input}</p>
                        </div>

                        {items.map((item) => (
                          <div key={item.itemId} className="item-block">
                            {item.kind === "assistant_message" ? (
                              <>
                                <span className="block-label">
                                  {zhCN.overview.assistantLabel}
                                </span>
                                <pre className="item-pre">{item.content}</pre>
                              </>
                            ) : item.kind === "command_execution" ? (
                              <>
                                <div className="item-header">
                                  <span className="block-label">
                                    {zhCN.overview.commandLabel}
                                  </span>
                                  {item.content.exitCode !== undefined ? (
                                    <span className="block-label tabular">
                                      {zhCN.overview.exitCodeLabel(
                                        item.content.exitCode,
                                      )}
                                    </span>
                                  ) : null}
                                </div>
                                <pre className="item-pre mono">
                                  $ {item.content.command}
                                  {item.content.output
                                    ? `\n${item.content.output}`
                                    : ""}
                                </pre>
                              </>
                            ) : (
                              <>
                                <div className="item-header">
                                  <span className="block-label">
                                    {zhCN.overview.fileChangeLabel}
                                  </span>
                                  <code className="block-label mono">
                                    {item.content.path}
                                  </code>
                                </div>
                                <pre className="item-pre mono">
                                  {item.content.diff}
                                </pre>
                              </>
                            )}
                          </div>
                        ))}
                      </div>
                    );
                  })
                )}
              </section>
            </>
          ) : null}
        </>
      )}
    </article>
  );
}
