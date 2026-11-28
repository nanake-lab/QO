import { useEffect, useMemo, useState } from "react";
import type {
  ApprovalDecideParams,
  ApprovalRequest,
  Choice,
  Item,
} from "@qo/protocol";
import {
  buildElicitationAnswer,
  createInitialElicitationValues,
  extractCommandApprovalDetails,
  extractFileApprovalDetails,
  extractPermissionsApprovalDetails,
  parseElicitation,
  type ElicitationFormValues,
} from "../elicitation.js";
import { formatRpcError, parseDiffLines } from "../format.js";
import { zhCN } from "../i18n/zh-CN.js";
import { isQoRpcError } from "../protocol.js";

interface ApprovalCardProps {
  readonly approval: ApprovalRequest;
  readonly linkedItem: Item | null;
  readonly isConnected: boolean;
  readonly onDecide: (
    request: ApprovalRequest,
    params: ApprovalDecideParams,
  ) => Promise<void>;
  readonly onRenew: (request: ApprovalRequest) => Promise<void>;
  readonly onRefreshList: (taskId: string) => Promise<void>;
}

/** 审批倒计时刷新周期（1 秒）：界面按秒展示剩余有效期。 */
const COUNTDOWN_TICK_MS = 1_000;

function formatRemaining(remainingMs: number): string {
  const totalSeconds = Math.max(0, Math.ceil(remainingMs / 1_000));
  const minutes = Math.floor(totalSeconds / 60);
  const seconds = totalSeconds % 60;
  if (minutes > 0) {
    return zhCN.approval.countdownMinutesSeconds(minutes, seconds);
  }
  return zhCN.approval.countdownSeconds(seconds);
}

export function ApprovalCard({
  approval,
  linkedItem,
  isConnected,
  onDecide,
  onRenew,
  onRefreshList,
}: ApprovalCardProps) {
  const expiresAtMs = useMemo(
    () => Date.parse(approval.expiresAt),
    [approval.expiresAt],
  );
  const [nowMs, setNowMs] = useState(() => Date.now());
  const [showRaw, setShowRaw] = useState(false);
  const [busy, setBusy] = useState<"allow_once" | "deny" | "renew" | null>(
    null,
  );
  const [error, setError] = useState<string | null>(null);
  const [fieldErrors, setFieldErrors] = useState<
    Readonly<Record<string, string>>
  >({});

  const parsedElicitation = useMemo(
    () =>
      approval.kind === "elicitation" ? parseElicitation(approval.raw) : null,
    [approval.kind, approval.raw],
  );

  const [formValues, setFormValues] = useState<ElicitationFormValues>(() =>
    parsedElicitation && parsedElicitation.supported
      ? createInitialElicitationValues(parsedElicitation.fields)
      : {},
  );

  useEffect(() => {
    setNowMs(Date.now());
    setShowRaw(false);
    setBusy(null);
    setError(null);
    setFieldErrors({});
    setFormValues(
      parsedElicitation && parsedElicitation.supported
        ? createInitialElicitationValues(parsedElicitation.fields)
        : {},
    );
  }, [approval.approvalId, parsedElicitation]);

  const remainingMs = Number.isFinite(expiresAtMs) ? expiresAtMs - nowMs : 0;
  const isExpired = approval.state === "expired" || remainingMs <= 0;

  useEffect(() => {
    if (isExpired) return;
    const handle = window.setInterval(() => {
      setNowMs(Date.now());
    }, COUNTDOWN_TICK_MS);
    return () => window.clearInterval(handle);
  }, [isExpired, approval.approvalId]);

  const formattedRaw = useMemo(
    () => (showRaw ? JSON.stringify(approval.raw, null, 2) : ""),
    [showRaw, approval.raw],
  );

  const handleDecision = async (choice: Choice) => {
    if (busy || isExpired) return;
    setError(null);
    setFieldErrors({});

    let answer: Record<string, unknown> | undefined;
    if (choice === "allow_once" && approval.kind === "elicitation") {
      if (!parsedElicitation || !parsedElicitation.supported) return;
      const built = buildElicitationAnswer(
        parsedElicitation.fields,
        formValues,
      );
      if (!built.ok) {
        setFieldErrors(built.fieldErrors);
        return;
      }
      answer = built.answer;
    }

    setBusy(choice);
    const params: ApprovalDecideParams = {
      taskId: approval.taskId,
      approvalId: approval.approvalId,
      decisionId: crypto.randomUUID(),
      digest: approval.digest,
      choice,
      ...(choice === "allow_once" && answer ? { answer } : {}),
    };

    try {
      await onDecide(approval, params);
    } catch (err: unknown) {
      if (isQoRpcError(err) && err.reason === "digest_mismatch") {
        setError(zhCN.approval.digestMismatchNotice);
        try {
          await onRefreshList(approval.taskId);
        } catch {
          // 保留摘要变更提示，避免覆盖主错误信息。
        }
      } else if (isQoRpcError(err) && err.reason === "request_withdrawn") {
        // 状态更新会将卡片移除并在对话流留下一行撤回提示。
      } else if (isQoRpcError(err) && err.reason === "approval_expired") {
        setError(null);
      } else {
        setError(formatRpcError(err));
      }
    } finally {
      setBusy(null);
    }
  };

  const handleRenew = async () => {
    if (busy) return;
    setError(null);
    setBusy("renew");
    try {
      await onRenew(approval);
    } catch (err: unknown) {
      if (isQoRpcError(err) && err.reason === "conflict") {
        try {
          await onRefreshList(approval.taskId);
        } catch {
          setError(formatRpcError(err));
        }
      } else if (isQoRpcError(err) && err.reason === "request_withdrawn") {
        // 卡片将被移除并在对话流留下一行撤回提示。
      } else {
        setError(formatRpcError(err));
      }
    } finally {
      setBusy(null);
    }
  };

  const canAllow =
    !isExpired &&
    (approval.kind !== "elicitation" ||
      (parsedElicitation !== null && parsedElicitation.supported));

  const renderDetails = () => {
    if (approval.kind === "command_execution") {
      const details = extractCommandApprovalDetails(approval, linkedItem);
      return (
        <div className="approval-details">
          {details.command ? (
            <div className="meta-field">
              <span className="meta-field-label">
                {zhCN.approval.commandLabel}
              </span>
              <div className="command-line mono">$ {details.command}</div>
            </div>
          ) : null}
          {details.cwd ? (
            <div className="path-row">
              <span className="path-label">{zhCN.approval.cwdLabel}</span>
              <code className="mono">{details.cwd}</code>
            </div>
          ) : null}
          {details.reason ? (
            <div className="meta-field">
              <span className="meta-field-label">
                {zhCN.approval.reasonLabel}
              </span>
              <span className="meta-field-value">{details.reason}</span>
            </div>
          ) : null}
        </div>
      );
    }

    if (approval.kind === "file_change") {
      const details = extractFileApprovalDetails(approval, linkedItem);
      const diffLines = details.diff ? parseDiffLines(details.diff) : [];
      return (
        <div className="approval-details">
          {details.path ? (
            <div className="path-row">
              <span className="path-label">{zhCN.approval.pathLabel}</span>
              <code className="file-path mono">{details.path}</code>
            </div>
          ) : null}
          {details.reason ? (
            <div className="meta-field">
              <span className="meta-field-label">
                {zhCN.approval.reasonLabel}
              </span>
              <span className="meta-field-value">{details.reason}</span>
            </div>
          ) : null}
          {diffLines.length > 0 ? (
            <div
              className="diff-view mono"
              role="region"
              aria-label={details.path ?? zhCN.approvalKind.file_change}
            >
              {diffLines.map((line, index) => (
                <div key={index} className={`diff-line diff-line-${line.kind}`}>
                  {line.text || " "}
                </div>
              ))}
            </div>
          ) : null}
        </div>
      );
    }

    if (approval.kind === "permissions") {
      const details = extractPermissionsApprovalDetails(approval);
      return (
        <div className="approval-details">
          {details.cwd ? (
            <div className="path-row">
              <span className="path-label">{zhCN.approval.cwdLabel}</span>
              <code className="mono">{details.cwd}</code>
            </div>
          ) : null}
          {details.reason ? (
            <div className="meta-field">
              <span className="meta-field-label">
                {zhCN.approval.reasonLabel}
              </span>
              <span className="meta-field-value">{details.reason}</span>
            </div>
          ) : null}
          <div className="meta-field">
            <span className="meta-field-label">
              {zhCN.approval.permissionsLabel}
            </span>
            {details.items.length > 0 ? (
              <ul className="permission-list">
                {details.items.map((entry, index) => (
                  <li key={index} className="permission-item mono">
                    {entry}
                  </li>
                ))}
              </ul>
            ) : (
              <p className="muted-hint">{zhCN.approval.permissionsEmpty}</p>
            )}
          </div>
        </div>
      );
    }

    if (!parsedElicitation) return null;

    if (!parsedElicitation.supported) {
      return (
        <div className="approval-details">
          {parsedElicitation.serverName ? (
            <div className="path-row">
              <span className="path-label">
                {zhCN.approval.elicitationServerLabel}
              </span>
              <code className="mono">{parsedElicitation.serverName}</code>
            </div>
          ) : null}
          {parsedElicitation.message ? (
            <p className="approval-message">{parsedElicitation.message}</p>
          ) : null}
          {parsedElicitation.url ? (
            <div className="path-row">
              <span className="path-label">
                {zhCN.approval.elicitationUrlLabel}
              </span>
              <code className="mono">{parsedElicitation.url}</code>
            </div>
          ) : null}
          <p className="unsupported-notice" role="status">
            {zhCN.approval.elicitationUnsupported}
          </p>
        </div>
      );
    }

    return (
      <div className="approval-details">
        {parsedElicitation.serverName ? (
          <div className="path-row">
            <span className="path-label">
              {zhCN.approval.elicitationServerLabel}
            </span>
            <code className="mono">{parsedElicitation.serverName}</code>
          </div>
        ) : null}
        {parsedElicitation.message ? (
          <p className="approval-message">{parsedElicitation.message}</p>
        ) : null}
        <div className="elicitation-fields">
          {parsedElicitation.fields.map((field) => {
            const fieldId = `elicit-${approval.approvalId}-${field.name}`;
            const fieldError = fieldErrors[field.name];
            if (field.kind === "boolean") {
              return (
                <div key={field.name} className="form-field">
                  <label className="filter-checkbox" htmlFor={fieldId}>
                    <input
                      id={fieldId}
                      type="checkbox"
                      checked={formValues[field.name] === true}
                      disabled={busy !== null || isExpired}
                      onChange={(e) =>
                        setFormValues((prev) => ({
                          ...prev,
                          [field.name]: e.currentTarget.checked,
                        }))
                      }
                    />
                    <span className="elicitation-bool-label">
                      {field.label}
                    </span>
                  </label>
                  {field.description ? (
                    <span className="muted-hint">{field.description}</span>
                  ) : null}
                </div>
              );
            }

            if (field.kind === "enum") {
              const current =
                typeof formValues[field.name] === "string"
                  ? (formValues[field.name] as string)
                  : "";
              return (
                <div key={field.name} className="form-field">
                  <label className="form-label" htmlFor={fieldId}>
                    {field.label}
                    {field.required ? (
                      <span className="required-tag">
                        {zhCN.approval.elicitationRequiredTag}
                      </span>
                    ) : null}
                  </label>
                  <select
                    id={fieldId}
                    className="form-input"
                    value={current}
                    disabled={busy !== null || isExpired}
                    onChange={(e) =>
                      setFormValues((prev) => ({
                        ...prev,
                        [field.name]: e.currentTarget.value,
                      }))
                    }
                  >
                    <option value="">
                      {zhCN.approval.elicitationSelectPlaceholder}
                    </option>
                    {field.options.map((opt) => (
                      <option key={opt.value} value={opt.value}>
                        {opt.label}
                      </option>
                    ))}
                  </select>
                  {field.description ? (
                    <span className="muted-hint">{field.description}</span>
                  ) : null}
                  {fieldError ? (
                    <span className="inline-error" role="alert">
                      {fieldError}
                    </span>
                  ) : null}
                </div>
              );
            }

            const current =
              typeof formValues[field.name] === "string"
                ? (formValues[field.name] as string)
                : "";
            return (
              <div key={field.name} className="form-field">
                <label className="form-label" htmlFor={fieldId}>
                  {field.label}
                  {field.required ? (
                    <span className="required-tag">
                      {zhCN.approval.elicitationRequiredTag}
                    </span>
                  ) : null}
                </label>
                <input
                  id={fieldId}
                  type={field.kind === "string" ? "text" : "number"}
                  step={field.kind === "integer" ? "1" : "any"}
                  min={
                    field.kind === "number" || field.kind === "integer"
                      ? field.minimum
                      : undefined
                  }
                  max={
                    field.kind === "number" || field.kind === "integer"
                      ? field.maximum
                      : undefined
                  }
                  className="form-input"
                  value={current}
                  disabled={busy !== null || isExpired}
                  onChange={(e) =>
                    setFormValues((prev) => ({
                      ...prev,
                      [field.name]: e.currentTarget.value,
                    }))
                  }
                />
                {field.description ? (
                  <span className="muted-hint">{field.description}</span>
                ) : null}
                {fieldError ? (
                  <span className="inline-error" role="alert">
                    {fieldError}
                  </span>
                ) : null}
              </div>
            );
          })}
        </div>
      </div>
    );
  };

  return (
    <section
      className="action-card"
      aria-label={zhCN.approvalKind[approval.kind]}
    >
      <div className="action-card-header">
        <span className="action-card-kind">
          {zhCN.approvalKind[approval.kind]}
        </span>
        <span
          className={
            isExpired
              ? "countdown-badge countdown-badge-expired tabular"
              : "countdown-badge tabular"
          }
        >
          {isExpired
            ? zhCN.approval.expiredBadge
            : formatRemaining(remainingMs)}
        </span>
      </div>

      <p className="approval-summary">{approval.summary}</p>

      {renderDetails()}

      <div className="raw-toggle-section">
        <button
          type="button"
          className="btn-link"
          aria-expanded={showRaw}
          onClick={() => setShowRaw((prev) => !prev)}
        >
          {showRaw ? zhCN.approval.hideRawButton : zhCN.approval.showRawButton}
        </button>
        {showRaw ? <pre className="item-pre mono">{formattedRaw}</pre> : null}
      </div>

      {(busy === "allow_once" || busy === "deny") && !isConnected ? (
        <p className="muted-hint" role="status">
          {zhCN.composer.reconnectingHint}
        </p>
      ) : null}

      {error ? (
        <p className="inline-error" role="alert">
          {error}
        </p>
      ) : null}

      <div className="action-card-footer">
        {isExpired ? (
          <button
            type="button"
            className="btn btn-action"
            disabled={!isConnected || busy !== null}
            onClick={() => void handleRenew()}
          >
            {busy === "renew"
              ? zhCN.approval.renewingButton
              : zhCN.approval.renewButton}
          </button>
        ) : (
          <>
            <button
              type="button"
              className="btn"
              disabled={busy !== null}
              onClick={() => void handleDecision("deny")}
            >
              {busy === "deny"
                ? isConnected
                  ? zhCN.approval.submittingButton
                  : zhCN.approval.queuedButton
                : zhCN.approval.denyButton}
            </button>
            {canAllow ? (
              <button
                type="button"
                className="btn btn-action"
                disabled={busy !== null}
                onClick={() => void handleDecision("allow_once")}
              >
                {busy === "allow_once"
                  ? isConnected
                    ? zhCN.approval.submittingButton
                    : zhCN.approval.queuedButton
                  : zhCN.approval.allowOnceButton}
              </button>
            ) : null}
          </>
        )}
      </div>
    </section>
  );
}
