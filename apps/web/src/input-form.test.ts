import { describe, expect, it, vi } from "vitest";
import type { InputQuestion } from "@qo/protocol";
import {
  OTHER_OPTION_SENTINEL,
  buildInputAnswerPayload,
  clearSecretDrafts,
  createInitialInputDrafts,
  scrubSecretAnswers,
} from "./input-form.js";
import {
  applyInputRequested,
  createInitialState,
  removeInputRequest,
} from "./state.js";

const QUESTIONS: [InputQuestion, ...InputQuestion[]] = [
  {
    id: "format",
    header: "输出格式",
    question: "使用哪种格式？",
    options: [
      { label: "文本", description: "普通文本" },
      { label: "JSON", description: "结构化输出" },
    ],
    allowOther: true,
    secret: false,
  },
  {
    id: "note",
    header: "备注",
    question: "请输入私有备注。",
    allowOther: true,
    secret: true,
  },
];

describe("input answer assembly and secret handling", () => {
  it("assembles { questionId: { answers: [...] } } for options, other text, and secret fields", () => {
    const initial = createInitialInputDrafts(QUESTIONS);
    expect(buildInputAnswerPayload(QUESTIONS, initial)).toMatchObject({
      ok: false,
      missingQuestionIds: ["format", "note"],
    });

    // 选择预设选项 + 填写保密题
    const withOption = buildInputAnswerPayload(QUESTIONS, {
      format: { selected: "文本", text: "" },
      note: { selected: OTHER_OPTION_SENTINEL, text: "top-secret-token" },
    });
    expect(withOption).toEqual({
      ok: true,
      answers: {
        format: { answers: ["文本"] },
        note: { answers: ["top-secret-token"] },
      },
    });

    // 选择「其他」并填写自定义回答
    const withOther = buildInputAnswerPayload(QUESTIONS, {
      format: { selected: OTHER_OPTION_SENTINEL, text: "YAML" },
      note: { selected: OTHER_OPTION_SENTINEL, text: "secret-2" },
    });
    expect(withOther).toEqual({
      ok: true,
      answers: {
        format: { answers: ["YAML"] },
        note: { answers: ["secret-2"] },
      },
    });
  });

  it("never writes secret answers to persistent storage or logs, and wipes them from memory after submit", () => {
    const SECRET_VALUE = "super-sensitive-api-passphrase-987654";

    const localSetItem = vi.fn();
    const sessionSetItem = vi.fn();
    const logSpy = vi.spyOn(console, "log").mockImplementation(() => {});
    const infoSpy = vi.spyOn(console, "info").mockImplementation(() => {});
    const warnSpy = vi.spyOn(console, "warn").mockImplementation(() => {});
    const errorSpy = vi.spyOn(console, "error").mockImplementation(() => {});

    const originalLocal = globalThis.localStorage;
    const originalSession = globalThis.sessionStorage;

    Object.defineProperty(globalThis, "localStorage", {
      configurable: true,
      value: {
        getItem: vi.fn(() => null),
        setItem: localSetItem,
        removeItem: vi.fn(),
        clear: vi.fn(),
        key: vi.fn(() => null),
        length: 0,
      },
    });
    Object.defineProperty(globalThis, "sessionStorage", {
      configurable: true,
      value: {
        getItem: vi.fn(() => null),
        setItem: sessionSetItem,
        removeItem: vi.fn(),
        clear: vi.fn(),
        key: vi.fn(() => null),
        length: 0,
      },
    });

    try {
      let state = applyInputRequested(createInitialState(), {
        requestId: "11111111-1111-4111-8111-111111111111",
        taskId: "22222222-2222-4222-8222-222222222222",
        sessionId: "33333333-3333-4333-8333-333333333333",
        turnId: "44444444-4444-4444-8444-444444444444",
        questions: QUESTIONS,
      });

      const drafts = {
        format: { selected: "JSON", text: "" },
        note: { selected: OTHER_OPTION_SENTINEL, text: SECRET_VALUE },
      };
      const built = buildInputAnswerPayload(QUESTIONS, drafts);
      expect(built.ok).toBe(true);
      if (!built.ok) return;

      // 模拟提交后立即清除内存中的保密字段与回答载荷
      const clearedDrafts = clearSecretDrafts(QUESTIONS, drafts);
      scrubSecretAnswers(QUESTIONS, built.answers);
      state = removeInputRequest(state, "11111111-1111-4111-8111-111111111111");

      expect(clearedDrafts.note.text).toBe("");
      expect(clearedDrafts.format.selected).toBe("JSON");
      expect(built.answers.note).toBeUndefined();
      expect(JSON.stringify(clearedDrafts)).not.toContain(SECRET_VALUE);
      expect(JSON.stringify(built.answers)).not.toContain(SECRET_VALUE);
      expect(JSON.stringify(state)).not.toContain(SECRET_VALUE);

      expect(localSetItem).not.toHaveBeenCalled();
      expect(sessionSetItem).not.toHaveBeenCalled();
      expect(logSpy).not.toHaveBeenCalled();
      expect(infoSpy).not.toHaveBeenCalled();
      expect(warnSpy).not.toHaveBeenCalled();
      expect(errorSpy).not.toHaveBeenCalled();
    } finally {
      Object.defineProperty(globalThis, "localStorage", {
        configurable: true,
        value: originalLocal,
      });
      Object.defineProperty(globalThis, "sessionStorage", {
        configurable: true,
        value: originalSession,
      });
      logSpy.mockRestore();
      infoSpy.mockRestore();
      warnSpy.mockRestore();
      errorSpy.mockRestore();
    }
  });
});
