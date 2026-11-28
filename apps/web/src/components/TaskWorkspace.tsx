import { useEffect, useState, type FormEvent } from "react";
import type {
  ApprovalDecideParams,
  ApprovalRequest,
  Id,
  InputAnswerParams,
  InputRequested,
  Session,
  SessionOpenParams,
  Task,
  TurnAppendParams,
  TurnInterruptParams,
  TurnStartParams,
} from "@qo/protocol";
import type { ConnectionStatus } from "../connection.js";
import { buildSessionOpenParams, formatRpcError } from "../format.js";
import { zhCN } from "../i18n/zh-CN.js";
import { isQoRpcError } from "../protocol.js";
import {
  getSessionApprovalDecisions,
  getSessionApprovals,
  getSessionInputs,
  getSessionTurns,
  getSessionWithdrawnNotices,
  getTaskSessions,
  isTerminalSessionState,
  type AppState,
} from "../state.js";
import { ApprovalCard } from "./ApprovalCard.js";
import { ConfirmDialog } from "./ConfirmDialog.js";
import { ConversationStream } from "./ConversationStream.js";
import { InputCard } from "./InputCard.js";
import { TurnComposer } from "./TurnComposer.js";

interface TaskWorkspaceProps {
  readonly state: AppState;
  readonly connectionStatus: ConnectionStatus;
  readonly selectedTask: Task | null;
  readonly selectedSessionId: Id | null;
  readonly onSelectSession: (sessionId: Id) => void;
  readonly onOpenCreateTask: () => void;
  readonly onOpenSession: (params: SessionOpenParams) => Promise<Session>;
  readonly onFinishSession: (taskId: Id, sessionId: Id) => Promise<void>;
  readonly onCancelSession: (taskId: Id, sessionId: Id) => Promise<void>;
  readonly onCloseTask: (taskId: Id) => Promise<void>;
  readonly onStartTurn: (params: TurnStartParams) => Promise<void>;
  readonly onAppendTurn: (params: TurnAppendParams) => Promise<void>;
  readonly onInterruptTurn: (params: TurnInterruptParams) => Promise<void>;
  readonly onDecideApproval: (
    request: ApprovalRequest,
    params: ApprovalDecideParams,
  ) => Promise<void>;
  readonly onRenewApproval: (request: ApprovalRequest) => Promise<void>;
  readonly onRefreshApprovals: (taskId: Id) => Promise<void>;
  readonly onAnswerInput: (
    request: InputRequested,
    params: InputAnswerParams,
  ) => Promise<void>;
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

export function TaskWorkspace({
  state,
  connectionStatus,
  selectedTask,
  selectedSessionId,
  onSelectSession,
  onOpenCreateTask,
  onOpenSession,
  onFinishSession,
  onCancelSession,
  onCloseTask,
  onStartTurn,
  onAppendTurn,
  onInterruptTurn,
  onDecideApproval,
  onRenewApproval,
  onRefreshApprovals,
  onAnswerInput,
}: TaskWorkspaceProps) {
  const [showOpenSession, setShowOpenSession] = useState(false);
  const [modelInput, setModelInput] = useState("");
  const [effortInput, setEffortInput] = useState("");
  const [tierInput, setTierInput] = useState("");
  const [openingSession, setOpeningSession] = useState(false);
  const [openSessionError, setOpenSessionError] = useState<string | null>(null);

  const [sessionActionBusy, setSessionActionBusy] = useState<
    "finish" | "cancel" | null
  >(null);
  const [sessionActionError, setSessionActionError] = useState<string | null>(
    null,
  );

  const [confirmExternalCancel, setConfirmExternalCancel] = useState(false);
  const [confirmCloseAll, setConfirmCloseAll] = useState(false);
  const [closeAllProgress, setCloseAllProgress] = useState<string | null>(null);
  const [closeAllError, setCloseAllError] = useState<string | null>(null);

  const taskId = selectedTask?.taskId ?? null;
  useEffect(() => {
    setShowOpenSession(false);
    setModelInput("");
    setEffortInput("");
    setTierInput("");
    setOpeningSession(false);
    setOpenSessionError(null);
    setSessionActionBusy(null);
    setSessionActionError(null);
    setConfirmExternalCancel(false);
    setConfirmCloseAll(false);
    setCloseAllProgress(null);
    setCloseAllError(null);
  }, [taskId]);

  useEffect(() => {
    setSessionActionBusy(null);
    setSessionActionError(null);
    setConfirmExternalCancel(false);
  }, [selectedSessionId]);

  const isConnected = connectionStatus.kind === "connected";
  const totalTasks = Object.keys(state.tasks).length;
  const isOwn = selectedTask?.createdBy === "ui";
  const sessions = selectedTask
    ? getTaskSessions(state, selectedTask.taskId)
    : [];
  const activeSession =
    sessions.find((s) => s.sessionId === selectedSessionId) ??
    sessions[0] ??
    null;
  const sessionApprovals =
    isOwn && activeSession
      ? getSessionApprovals(state, activeSession.sessionId)
      : [];

  // 自己的会话处于 waiting_approval 但内存无对应审批卡（如离线期间已过期未重发）时，用 approval.list 拉回。
  const needsApprovalSync =
    isConnected &&
    isOwn &&
    activeSession !== null &&
    activeSession.state === "waiting_approval" &&
    sessionApprovals.length === 0;
  const syncSessionId = activeSession?.sessionId ?? null;
  useEffect(() => {
    if (!needsApprovalSync || !taskId || !syncSessionId) return;
    void onRefreshApprovals(taskId).catch(() => {
      // 网络波动时忽略，下次状态变化或重连会再次同步。
    });
  }, [needsApprovalSync, taskId, syncSessionId, onRefreshApprovals]);

  if (!selectedTask) {
    let title: string = zhCN.overview.noSelectionTitle;
    let body: string = zhCN.overview.noSelectionBody;
    let showCreateAction = false;

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
        showCreateAction = true;
      }
    }

    return (
      <section className="empty-state">
        <h2 className="empty-state-title">{title}</h2>
        <p className="empty-state-body">{body}</p>
        {showCreateAction ? (
          <div className="empty-state-actions">
            <button
              type="button"
              className="btn btn-primary"
              onClick={onOpenCreateTask}
            >
              {zhCN.sidebar.newTaskButton}
            </button>
          </div>
        ) : null}
      </section>
    );
  }

  const isClosed = selectedTask.state === "closed";
  const canManageTask = isOwn && !isClosed;

  const activeSessions = sessions.filter(
    (s) => !isTerminalSessionState(s.state),
  );
  const turns = activeSession
    ? getSessionTurns(state, activeSession.sessionId)
    : [];
  const runningTurn = turns.find((t) => t.state === "running") ?? null;
  const sessionInputs =
    isOwn && activeSession
      ? getSessionInputs(state, activeSession.sessionId)
      : [];
  const sessionApprovalDecisions = activeSession
    ? getSessionApprovalDecisions(state, activeSession.sessionId)
    : [];
  const sessionWithdrawnNotices = activeSession
    ? getSessionWithdrawnNotices(state, activeSession.sessionId)
    : [];

  const handleOpenSessionSubmit = async (event: FormEvent) => {
    event.preventDefault();
    if (openingSession || !canManageTask) return;
    setOpenSessionError(null);
    setOpeningSession(true);
    const params = buildSessionOpenParams(selectedTask.taskId, {
      model: modelInput,
      reasoningEffort: effortInput,
      serviceTier: tierInput,
    });
    try {
      const created = await onOpenSession(params);
      onSelectSession(created.sessionId);
      setModelInput("");
      setEffortInput("");
      setTierInput("");
      setShowOpenSession(false);
    } catch (err: unknown) {
      setOpenSessionError(formatRpcError(err));
    } finally {
      setOpeningSession(false);
    }
  };

  const handleFinishSession = async () => {
    if (!activeSession || !canManageTask || sessionActionBusy) return;
    setSessionActionError(null);
    setSessionActionBusy("finish");
    try {
      await onFinishSession(selectedTask.taskId, activeSession.sessionId);
    } catch (err: unknown) {
      setSessionActionError(formatRpcError(err));
    } finally {
      setSessionActionBusy(null);
    }
  };

  const executeCancelSession = async () => {
    if (!activeSession || isClosed || sessionActionBusy) return;
    setSessionActionError(null);
    setSessionActionBusy("cancel");
    try {
      await onCancelSession(selectedTask.taskId, activeSession.sessionId);
      setConfirmExternalCancel(false);
    } catch (err: unknown) {
      setSessionActionError(formatRpcError(err));
    } finally {
      setSessionActionBusy(null);
    }
  };

  const handleCancelClick = () => {
    if (!activeSession || isClosed || sessionActionBusy) return;
    if (selectedTask.createdBy === "external") {
      setSessionActionError(null);
      setConfirmExternalCancel(true);
    } else {
      void executeCancelSession();
    }
  };

  const executeCloseAll = async () => {
    if (!canManageTask || closeAllProgress !== null) return;
    setCloseAllError(null);
    setConfirmCloseAll(false);

    const targets = sessions.filter((s) => !isTerminalSessionState(s.state));
    try {
      for (let i = 0; i < targets.length; i += 1) {
        setCloseAllProgress(
          zhCN.taskActions.cancellingProgress(i + 1, targets.length),
        );
        try {
          await onCancelSession(selectedTask.taskId, targets[i].sessionId);
        } catch (err: unknown) {
          if (!isQoRpcError(err) || err.reason !== "conflict") {
            throw err;
          }
          // 若会话在并发中已进入终态，继续由 task.close 做最终一致性校验。
        }
      }
      setCloseAllProgress(zhCN.taskActions.closingProgress);
      await onCloseTask(selectedTask.taskId);
      setCloseAllProgress(null);
    } catch (err: unknown) {
      setCloseAllProgress(null);
      if (isQoRpcError(err) && err.reason === "conflict") {
        setCloseAllError(zhCN.taskActions.closeConflict);
      } else {
        setCloseAllError(formatRpcError(err));
      }
    }
  };

  const canCancelActive =
    !isClosed &&
    activeSession !== null &&
    !isTerminalSessionState(activeSession.state);
  const canFinishActive =
    canManageTask && activeSession !== null && activeSession.state === "idle";

  return (
    <article className="task-overview">
      <header className="overview-header">
        <div className="overview-header-top">
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

          {canManageTask ? (
            <div className="header-actions">
              <button
                type="button"
                className="btn"
                disabled={closeAllProgress !== null}
                onClick={() => {
                  setOpenSessionError(null);
                  setShowOpenSession((prev) => !prev);
                }}
              >
                {showOpenSession
                  ? zhCN.openSession.cancel
                  : zhCN.openSession.toggleButton}
              </button>
              <button
                type="button"
                className="btn"
                disabled={!isConnected || closeAllProgress !== null}
                onClick={() => {
                  setCloseAllError(null);
                  setConfirmCloseAll(true);
                }}
              >
                {zhCN.taskActions.closeAllButton}
              </button>
            </div>
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

      {closeAllProgress ? (
        <div className="notice-bar tabular" role="status" aria-live="polite">
          {closeAllProgress}
        </div>
      ) : null}

      {closeAllError ? (
        <div className="notice-bar" role="alert">
          <span className="inline-error">{closeAllError}</span>
        </div>
      ) : null}

      {isClosed ? (
        <div className="notice-bar">{zhCN.overview.closedNotice}</div>
      ) : null}

      {selectedTask.createdBy === "external" ? (
        <div className="notice-bar">{zhCN.overview.externalNotice}</div>
      ) : null}

      {canManageTask && showOpenSession ? (
        <form
          className="open-session-panel"
          onSubmit={(e) => void handleOpenSessionSubmit(e)}
        >
          <div className="open-session-grid">
            <label className="form-field">
              <span className="form-label">{zhCN.openSession.modelLabel}</span>
              <input
                type="text"
                className="form-input mono"
                value={modelInput}
                placeholder={zhCN.openSession.modelPlaceholder}
                disabled={openingSession}
                onChange={(e) => setModelInput(e.currentTarget.value)}
              />
            </label>
            <label className="form-field">
              <span className="form-label">
                {zhCN.openSession.reasoningEffortLabel}
              </span>
              <input
                type="text"
                className="form-input"
                value={effortInput}
                placeholder={zhCN.openSession.reasoningEffortPlaceholder}
                disabled={openingSession}
                onChange={(e) => setEffortInput(e.currentTarget.value)}
              />
            </label>
            <label className="form-field">
              <span className="form-label">
                {zhCN.openSession.serviceTierLabel}
              </span>
              <input
                type="text"
                className="form-input"
                value={tierInput}
                placeholder={zhCN.openSession.serviceTierPlaceholder}
                disabled={openingSession}
                onChange={(e) => setTierInput(e.currentTarget.value)}
              />
            </label>
          </div>

          {openingSession && !isConnected ? (
            <p className="muted-hint" role="status">
              {zhCN.composer.reconnectingHint}
            </p>
          ) : null}

          {openSessionError ? (
            <p className="inline-error" role="alert">
              {openSessionError}
            </p>
          ) : null}

          <div className="panel-actions">
            <button
              type="button"
              className="btn"
              disabled={openingSession}
              onClick={() => setShowOpenSession(false)}
            >
              {zhCN.openSession.cancel}
            </button>
            <button
              type="submit"
              className="btn btn-primary"
              disabled={openingSession}
            >
              {openingSession
                ? isConnected
                  ? zhCN.openSession.submitting
                  : zhCN.openSession.queued
                : zhCN.openSession.submit}
            </button>
          </div>
        </form>
      ) : null}

      {sessions.length === 0 ? (
        <p className="empty-state-body">
          {isClosed
            ? zhCN.overview.noSessionsClosedBody
            : zhCN.overview.noSessionsBody}
        </p>
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
                isOwn &&
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
                        : session.state === "failed"
                          ? "status-text-failed"
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
              {isOwn &&
              activeSession.state === "waiting_approval" &&
              sessionApprovals.length === 0 ? (
                <div className="notice-bar notice-bar-action" role="status">
                  {zhCN.approval.loadingPending}
                </div>
              ) : null}

              {isOwn &&
              activeSession.state === "waiting_input" &&
              sessionInputs.length === 0 ? (
                <div className="notice-bar notice-bar-action" role="status">
                  {zhCN.overview.waitingYourInput}
                </div>
              ) : null}

              {!isOwn &&
              (activeSession.state === "waiting_approval" ||
                activeSession.state === "waiting_input") ? (
                <div className="notice-bar" role="status">
                  {zhCN.overview.waitingExternalApproval}
                </div>
              ) : null}

              <section className="session-card">
                <div className="session-card-top">
                  <div className="session-status-line">
                    <span className="meta-field-label">
                      {zhCN.overview.sessionStatusLabel}
                    </span>
                    <span
                      className={
                        activeSession.state === "failed"
                          ? "status-text-failed"
                          : isOwn &&
                              (activeSession.state === "waiting_approval" ||
                                activeSession.state === "waiting_input")
                            ? "status-text-action"
                            : "meta-field-value"
                      }
                    >
                      {sessionStatusText(selectedTask, activeSession)}
                      {activeSession.cancelledBy
                        ? ` · ${zhCN.cancelledBy[activeSession.cancelledBy]}`
                        : ""}
                    </span>
                  </div>

                  {canFinishActive || canCancelActive ? (
                    <div className="session-actions">
                      {canFinishActive ? (
                        <button
                          type="button"
                          className="btn"
                          disabled={
                            !isConnected ||
                            sessionActionBusy !== null ||
                            closeAllProgress !== null
                          }
                          onClick={() => void handleFinishSession()}
                        >
                          {sessionActionBusy === "finish"
                            ? zhCN.taskActions.finishingSession
                            : zhCN.taskActions.finishSession}
                        </button>
                      ) : null}
                      {canCancelActive ? (
                        <button
                          type="button"
                          className="btn"
                          disabled={
                            !isConnected ||
                            sessionActionBusy !== null ||
                            closeAllProgress !== null
                          }
                          onClick={handleCancelClick}
                        >
                          {sessionActionBusy === "cancel"
                            ? zhCN.taskActions.cancellingSession
                            : zhCN.taskActions.cancelSession}
                        </button>
                      ) : null}
                    </div>
                  ) : null}
                </div>

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
                      {activeSession.model ?? zhCN.overview.unreportedValue}
                    </span>
                  </div>
                  <div className="meta-field">
                    <span className="meta-field-label">
                      {zhCN.overview.reasoningEffortLabel}
                    </span>
                    <span className="meta-field-value">
                      {activeSession.reasoningEffort ??
                        zhCN.overview.unreportedValue}
                    </span>
                  </div>
                  <div className="meta-field">
                    <span className="meta-field-label">
                      {zhCN.overview.serviceTierLabel}
                    </span>
                    <span className="meta-field-value">
                      {activeSession.serviceTier ??
                        zhCN.overview.unreportedValue}
                    </span>
                  </div>
                </div>

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

                {sessionActionError && !confirmExternalCancel ? (
                  <p className="inline-error" role="alert">
                    {sessionActionError}
                  </p>
                ) : null}
              </section>

              {isOwn && sessionApprovals.length > 0 ? (
                <div className="action-cards-stack">
                  {sessionApprovals.map((approval) => (
                    <ApprovalCard
                      key={approval.approvalId}
                      approval={approval}
                      linkedItem={
                        approval.itemId
                          ? (state.items[approval.itemId] ?? null)
                          : null
                      }
                      isConnected={isConnected}
                      onDecide={onDecideApproval}
                      onRenew={onRenewApproval}
                      onRefreshList={onRefreshApprovals}
                    />
                  ))}
                </div>
              ) : null}

              {isOwn && sessionInputs.length > 0 ? (
                <div className="action-cards-stack">
                  {sessionInputs.map((inputReq) => (
                    <InputCard
                      key={inputReq.requestId}
                      request={inputReq}
                      isConnected={isConnected}
                      onSubmit={onAnswerInput}
                    />
                  ))}
                </div>
              ) : null}

              <ConversationStream
                state={state}
                turns={turns}
                approvalDecisions={sessionApprovalDecisions}
                withdrawnNotices={sessionWithdrawnNotices}
              />

              {canManageTask && !isTerminalSessionState(activeSession.state) ? (
                <TurnComposer
                  session={activeSession}
                  runningTurn={runningTurn}
                  isConnected={isConnected}
                  onStartTurn={onStartTurn}
                  onAppendTurn={onAppendTurn}
                  onInterruptTurn={onInterruptTurn}
                />
              ) : null}
            </>
          ) : null}
        </>
      )}

      {confirmExternalCancel ? (
        <ConfirmDialog
          title={zhCN.taskActions.externalCancelTitle}
          body={zhCN.taskActions.externalCancelBody}
          confirmLabel={
            sessionActionBusy === "cancel"
              ? zhCN.taskActions.cancellingSession
              : zhCN.taskActions.externalCancelConfirm
          }
          cancelLabel={zhCN.taskActions.externalCancelDismiss}
          busy={sessionActionBusy === "cancel"}
          error={sessionActionError}
          onConfirm={() => void executeCancelSession()}
          onCancel={() => {
            setSessionActionError(null);
            setConfirmExternalCancel(false);
          }}
        />
      ) : null}

      {confirmCloseAll ? (
        <ConfirmDialog
          title={zhCN.taskActions.closeAllConfirmTitle}
          body={zhCN.taskActions.closeAllConfirmBody(activeSessions.length)}
          confirmLabel={zhCN.taskActions.closeAllConfirmAction}
          cancelLabel={zhCN.taskActions.closeAllCancelAction}
          onConfirm={() => void executeCloseAll()}
          onCancel={() => setConfirmCloseAll(false)}
        />
      ) : null}
    </article>
  );
}
