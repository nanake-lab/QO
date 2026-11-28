import { useEffect, useRef, useState, type FormEvent } from "react";
import type { TaskCreateParams } from "@qo/protocol";
import { buildTaskCreateParams, formatRpcError } from "../format.js";
import { zhCN } from "../i18n/zh-CN.js";

interface CreateTaskDialogProps {
  readonly isConnected: boolean;
  readonly onCreate: (params: TaskCreateParams) => Promise<void>;
  readonly onClose: () => void;
}

export function CreateTaskDialog({
  isConnected,
  onCreate,
  onClose,
}: CreateTaskDialogProps) {
  const [name, setName] = useState("");
  const [group, setGroup] = useState("");
  const [cwd, setCwd] = useState("");
  const [forbiddenRootsText, setForbiddenRootsText] = useState("");
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const nameInputRef = useRef<HTMLInputElement | null>(null);

  useEffect(() => {
    nameInputRef.current?.focus();
  }, []);

  useEffect(() => {
    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape" && !submitting) {
        event.preventDefault();
        onClose();
      }
    };
    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [submitting, onClose]);

  const handleSubmit = async (event: FormEvent) => {
    event.preventDefault();
    if (submitting) return;
    if (!name.trim()) {
      setError(zhCN.createTask.nameRequired);
      nameInputRef.current?.focus();
      return;
    }

    setError(null);
    setSubmitting(true);
    const params = buildTaskCreateParams({
      name,
      group,
      cwd,
      forbiddenRootsText,
    });
    try {
      await onCreate(params);
      onClose();
    } catch (err: unknown) {
      setError(formatRpcError(err));
      setSubmitting(false);
    }
  };

  return (
    <div
      className="dialog-backdrop"
      onClick={(event) => {
        if (event.target === event.currentTarget && !submitting) {
          onClose();
        }
      }}
    >
      <div
        className="dialog-card dialog-card-wide"
        role="dialog"
        aria-modal="true"
        aria-labelledby="create-task-title"
      >
        <h2 id="create-task-title" className="dialog-title">
          {zhCN.createTask.title}
        </h2>

        <form className="dialog-form" onSubmit={(e) => void handleSubmit(e)}>
          <label className="form-field">
            <span className="form-label">{zhCN.createTask.nameLabel}</span>
            <input
              ref={nameInputRef}
              type="text"
              className="form-input"
              value={name}
              placeholder={zhCN.createTask.namePlaceholder}
              disabled={submitting}
              onChange={(e) => setName(e.currentTarget.value)}
            />
          </label>

          <label className="form-field">
            <span className="form-label">{zhCN.createTask.groupLabel}</span>
            <input
              type="text"
              className="form-input"
              value={group}
              placeholder={zhCN.createTask.groupPlaceholder}
              disabled={submitting}
              onChange={(e) => setGroup(e.currentTarget.value)}
            />
          </label>

          <label className="form-field">
            <span className="form-label">{zhCN.createTask.cwdLabel}</span>
            <input
              type="text"
              className="form-input mono"
              value={cwd}
              placeholder={zhCN.createTask.cwdPlaceholder}
              disabled={submitting}
              onChange={(e) => setCwd(e.currentTarget.value)}
            />
          </label>

          <label className="form-field">
            <span className="form-label">
              {zhCN.createTask.forbiddenRootsLabel}
            </span>
            <textarea
              className="form-textarea mono"
              rows={3}
              value={forbiddenRootsText}
              placeholder={zhCN.createTask.forbiddenRootsPlaceholder}
              disabled={submitting}
              onChange={(e) => setForbiddenRootsText(e.currentTarget.value)}
            />
          </label>

          {submitting && !isConnected ? (
            <p className="muted-hint" role="status">
              {zhCN.composer.reconnectingHint}
            </p>
          ) : null}

          {error ? (
            <p className="inline-error" role="alert">
              {error}
            </p>
          ) : null}

          <div className="dialog-actions">
            <button
              type="button"
              className="btn"
              disabled={submitting}
              onClick={onClose}
            >
              {zhCN.createTask.cancel}
            </button>
            <button
              type="submit"
              className="btn btn-primary"
              disabled={submitting}
            >
              {submitting
                ? isConnected
                  ? zhCN.createTask.submitting
                  : zhCN.createTask.queued
                : zhCN.createTask.submit}
            </button>
          </div>
        </form>
      </div>
    </div>
  );
}
