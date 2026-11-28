import { useEffect, useState, type FormEvent } from "react";
import type { InputAnswerParams, InputRequested } from "@qo/protocol";
import { formatRpcError } from "../format.js";
import { zhCN } from "../i18n/zh-CN.js";
import {
  OTHER_OPTION_SENTINEL,
  buildInputAnswerPayload,
  clearSecretDrafts,
  createInitialInputDrafts,
  scrubSecretAnswers,
  type InputDraftMap,
} from "../input-form.js";
import { isQoRpcError } from "../protocol.js";

interface InputCardProps {
  readonly request: InputRequested;
  readonly isConnected: boolean;
  readonly onSubmit: (
    request: InputRequested,
    params: InputAnswerParams,
  ) => Promise<void>;
}

export function InputCard({ request, isConnected, onSubmit }: InputCardProps) {
  const [drafts, setDrafts] = useState<InputDraftMap>(() =>
    createInitialInputDrafts(request.questions),
  );
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    setDrafts(createInitialInputDrafts(request.questions));
    setSubmitting(false);
    setError(null);
    return () => {
      setDrafts((prev) => clearSecretDrafts(request.questions, prev));
    };
  }, [request.requestId, request.questions]);

  const handleSubmit = async (event: FormEvent) => {
    event.preventDefault();
    if (submitting) return;
    setError(null);

    const built = buildInputAnswerPayload(request.questions, drafts);
    if (!built.ok) {
      setError(built.message);
      return;
    }

    setSubmitting(true);
    const params: InputAnswerParams = {
      sessionId: request.sessionId,
      turnId: request.turnId,
      requestId: request.requestId,
      answers: built.answers,
    };

    try {
      await onSubmit(request, params);
      setDrafts((prev) => clearSecretDrafts(request.questions, prev));
    } catch (err: unknown) {
      if (isQoRpcError(err) && err.reason === "request_withdrawn") {
        setDrafts((prev) => clearSecretDrafts(request.questions, prev));
      } else {
        setError(formatRpcError(err));
      }
    } finally {
      scrubSecretAnswers(request.questions, built.answers);
      setSubmitting(false);
    }
  };

  return (
    <section className="action-card" aria-label={zhCN.input.heading}>
      <div className="action-card-header">
        <span className="action-card-kind">{zhCN.input.heading}</span>
        <span className="countdown-badge tabular">
          {zhCN.input.questionCount(request.questions.length)}
        </span>
      </div>

      <form className="input-card-form" onSubmit={(e) => void handleSubmit(e)}>
        {request.questions.map((question) => {
          const draft = drafts[question.id] ?? { selected: "", text: "" };
          const hasOptions =
            Array.isArray(question.options) && question.options.length > 0;
          const groupName = `qo-input-${request.requestId}-${question.id}`;
          const textInputId = `${groupName}-text`;

          return (
            <fieldset key={question.id} className="input-question-group">
              <legend className="input-question-header">
                <span>{question.header}</span>
                {question.secret ? (
                  <span className="meta-pill">{zhCN.input.secretBadge}</span>
                ) : null}
              </legend>
              <p className="input-question-text">{question.question}</p>

              {hasOptions ? (
                <div className="input-options-list">
                  {question.options!.map((opt) => (
                    <label key={opt.label} className="input-option-row">
                      <input
                        type="radio"
                        name={groupName}
                        value={opt.label}
                        checked={draft.selected === opt.label}
                        disabled={submitting}
                        onChange={() =>
                          setDrafts((prev) => ({
                            ...prev,
                            [question.id]: {
                              ...draft,
                              selected: opt.label,
                            },
                          }))
                        }
                      />
                      <span className="input-option-body">
                        <span className="input-option-label">{opt.label}</span>
                        {opt.description ? (
                          <span className="input-option-desc">
                            {opt.description}
                          </span>
                        ) : null}
                      </span>
                    </label>
                  ))}

                  {question.allowOther ? (
                    <div className="input-other-block">
                      <label className="input-option-row">
                        <input
                          type="radio"
                          name={groupName}
                          value={OTHER_OPTION_SENTINEL}
                          checked={draft.selected === OTHER_OPTION_SENTINEL}
                          disabled={submitting}
                          onChange={() =>
                            setDrafts((prev) => ({
                              ...prev,
                              [question.id]: {
                                ...draft,
                                selected: OTHER_OPTION_SENTINEL,
                              },
                            }))
                          }
                        />
                        <span className="input-option-label">
                          {zhCN.input.otherOptionLabel}
                        </span>
                      </label>
                      <input
                        id={textInputId}
                        type={question.secret ? "password" : "text"}
                        autoComplete="off"
                        spellCheck={false}
                        className="form-input"
                        value={draft.text}
                        placeholder={
                          question.secret
                            ? zhCN.input.secretPlaceholder
                            : zhCN.input.otherPlaceholder
                        }
                        disabled={submitting}
                        aria-label={`${question.header} ${zhCN.input.otherOptionLabel}`}
                        onFocus={() => {
                          if (draft.selected !== OTHER_OPTION_SENTINEL) {
                            setDrafts((prev) => ({
                              ...prev,
                              [question.id]: {
                                ...draft,
                                selected: OTHER_OPTION_SENTINEL,
                              },
                            }));
                          }
                        }}
                        onChange={(e) => {
                          const value = e.currentTarget.value;
                          setDrafts((prev) => ({
                            ...prev,
                            [question.id]: {
                              selected: OTHER_OPTION_SENTINEL,
                              text: value,
                            },
                          }));
                        }}
                      />
                    </div>
                  ) : null}
                </div>
              ) : (
                <input
                  id={textInputId}
                  type={question.secret ? "password" : "text"}
                  autoComplete="off"
                  spellCheck={false}
                  className="form-input"
                  value={draft.text}
                  placeholder={
                    question.secret
                      ? zhCN.input.secretPlaceholder
                      : zhCN.input.textPlaceholder
                  }
                  disabled={submitting}
                  aria-label={question.header}
                  onChange={(e) => {
                    const value = e.currentTarget.value;
                    setDrafts((prev) => ({
                      ...prev,
                      [question.id]: {
                        selected: OTHER_OPTION_SENTINEL,
                        text: value,
                      },
                    }));
                  }}
                />
              )}
            </fieldset>
          );
        })}

        {error ? (
          <p className="inline-error" role="alert">
            {error}
          </p>
        ) : null}

        <div className="action-card-footer">
          <button
            type="submit"
            className="btn btn-action"
            disabled={!isConnected || submitting}
          >
            {submitting ? zhCN.input.submittingButton : zhCN.input.submitButton}
          </button>
        </div>
      </form>
    </section>
  );
}
