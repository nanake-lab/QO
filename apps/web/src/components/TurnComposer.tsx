import { useEffect, useRef, useState, type KeyboardEvent } from "react";
import type {
  Session,
  Turn,
  TurnAppendParams,
  TurnInterruptParams,
  TurnStartParams,
} from "@qo/protocol";
import { formatRpcError } from "../format.js";
import { zhCN } from "../i18n/zh-CN.js";

interface TurnComposerProps {
  readonly session: Session;
  readonly runningTurn: Turn | null;
  readonly isConnected: boolean;
  readonly onStartTurn: (params: TurnStartParams) => Promise<void>;
  readonly onAppendTurn: (params: TurnAppendParams) => Promise<void>;
  readonly onInterruptTurn: (params: TurnInterruptParams) => Promise<void>;
}

export function TurnComposer({
  session,
  runningTurn,
  isConnected,
  onStartTurn,
  onAppendTurn,
  onInterruptTurn,
}: TurnComposerProps) {
  const [text, setText] = useState("");
  const [submitting, setSubmitting] = useState(false);
  const [interrupting, setInterrupting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const composingRef = useRef(false);

  useEffect(() => {
    setText("");
    setError(null);
    setSubmitting(false);
    setInterrupting(false);
  }, [session.sessionId]);

  const isStarting = session.state === "starting";
  const isRunning = runningTurn !== null;
  const canSubmit = !submitting && !isStarting && text.trim().length > 0;

  const submit = async () => {
    const trimmed = text.trim();
    if (!trimmed || submitting || isStarting) return;

    setError(null);
    setSubmitting(true);
    try {
      if (runningTurn) {
        await onAppendTurn({
          sessionId: session.sessionId,
          turnId: runningTurn.turnId,
          appendId: crypto.randomUUID(),
          input: trimmed,
        });
      } else {
        await onStartTurn({
          sessionId: session.sessionId,
          turnId: crypto.randomUUID(),
          input: trimmed,
        });
      }
      setText("");
    } catch (err: unknown) {
      setError(formatRpcError(err));
    } finally {
      setSubmitting(false);
    }
  };

  const handleInterrupt = async () => {
    if (!runningTurn || interrupting) return;
    setError(null);
    setInterrupting(true);
    try {
      await onInterruptTurn({
        sessionId: session.sessionId,
        turnId: runningTurn.turnId,
      });
    } catch (err: unknown) {
      setError(formatRpcError(err));
    } finally {
      setInterrupting(false);
    }
  };

  const handleKeyDown = (event: KeyboardEvent<HTMLTextAreaElement>) => {
    if (event.key !== "Enter" || event.shiftKey) return;
    if (
      event.nativeEvent.isComposing ||
      event.keyCode === 229 ||
      composingRef.current
    ) {
      return;
    }
    event.preventDefault();
    void submit();
  };

  const placeholder = isStarting
    ? zhCN.composer.startingPlaceholder
    : isRunning
      ? zhCN.composer.appendPlaceholder
      : zhCN.composer.startPlaceholder;

  const submitLabel = submitting
    ? isRunning
      ? zhCN.composer.appendingButton
      : zhCN.composer.sendingButton
    : isRunning
      ? zhCN.composer.appendButton
      : zhCN.composer.sendButton;

  return (
    <section className="composer" aria-label={zhCN.composer.label}>
      <textarea
        className="composer-textarea"
        rows={3}
        value={text}
        placeholder={placeholder}
        disabled={isStarting || submitting}
        aria-label={zhCN.composer.label}
        onChange={(e) => setText(e.currentTarget.value)}
        onCompositionStart={() => {
          composingRef.current = true;
        }}
        onCompositionEnd={() => {
          composingRef.current = false;
        }}
        onKeyDown={handleKeyDown}
      />

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

      <div className="composer-actions">
        {isRunning ? (
          <button
            type="button"
            className="btn"
            disabled={!isConnected || interrupting}
            onClick={() => void handleInterrupt()}
          >
            {interrupting
              ? zhCN.composer.interruptingButton
              : zhCN.composer.interruptButton}
          </button>
        ) : null}
        <button
          type="button"
          className="btn btn-primary"
          disabled={!canSubmit}
          onClick={() => void submit()}
        >
          {submitLabel}
        </button>
      </div>
    </section>
  );
}
