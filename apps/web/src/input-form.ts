import type { InputAnswer, InputQuestion } from "@qo/protocol";
import { zhCN } from "./i18n/zh-CN.js";

export const OTHER_OPTION_SENTINEL = "__qo_other__";

export interface QuestionDraft {
  /** 选中的选项 label，或 OTHER_OPTION_SENTINEL 表示选择「其他」 */
  readonly selected: string;
  /** 「其他」或纯文本题的输入值 */
  readonly text: string;
}

export type InputDraftMap = Readonly<Record<string, QuestionDraft>>;

export type InputAnswerBuildResult =
  | { readonly ok: true; readonly answers: Record<string, InputAnswer> }
  | {
      readonly ok: false;
      readonly missingQuestionIds: readonly string[];
      readonly message: string;
    };

export function createInitialInputDrafts(
  questions: readonly InputQuestion[],
): InputDraftMap {
  const drafts: Record<string, QuestionDraft> = {};
  for (const question of questions) {
    const hasOptions =
      Array.isArray(question.options) && question.options.length > 0;
    drafts[question.id] = {
      selected: hasOptions ? "" : OTHER_OPTION_SENTINEL,
      text: "",
    };
  }
  return drafts;
}

/**
 * 组装 input.answer 所需的 `{ "<题目id>": { "answers": ["…"] } }` 载荷。
 * 不写入任何持久存储或日志。
 */
export function buildInputAnswerPayload(
  questions: readonly InputQuestion[],
  drafts: InputDraftMap,
): InputAnswerBuildResult {
  const answers: Record<string, InputAnswer> = {};
  const missingQuestionIds: string[] = [];

  for (const question of questions) {
    const draft = drafts[question.id] ?? { selected: "", text: "" };
    const hasOptions =
      Array.isArray(question.options) && question.options.length > 0;

    let value: string;
    if (hasOptions) {
      if (draft.selected === OTHER_OPTION_SENTINEL) {
        value = question.allowOther ? draft.text.trim() : "";
      } else {
        const validOption = question.options!.some(
          (opt) => opt.label === draft.selected,
        );
        value = validOption ? draft.selected : "";
      }
    } else {
      value = draft.text.trim();
    }

    if (value.length === 0) {
      missingQuestionIds.push(question.id);
    } else {
      answers[question.id] = { answers: [value] };
    }
  }

  if (missingQuestionIds.length > 0) {
    return {
      ok: false,
      missingQuestionIds,
      message: zhCN.input.answerRequired,
    };
  }
  return { ok: true, answers };
}

/**
 * 立即从内存草稿中清掉所有 secret 题目的回答。
 */
export function clearSecretDrafts(
  questions: readonly InputQuestion[],
  drafts: InputDraftMap,
): InputDraftMap {
  let changed = false;
  const next: Record<string, QuestionDraft> = { ...drafts };
  for (const question of questions) {
    if (!question.secret) continue;
    const current = next[question.id];
    if (current && (current.text !== "" || current.selected !== "")) {
      const hasOptions =
        Array.isArray(question.options) && question.options.length > 0;
      next[question.id] = {
        selected: hasOptions ? "" : OTHER_OPTION_SENTINEL,
        text: "",
      };
      changed = true;
    }
  }
  return changed ? next : drafts;
}

/**
 * 立即覆写载荷中所有 secret 题目的回答数组，避免敏感内容残留在内存引用里。
 */
export function scrubSecretAnswers(
  questions: readonly InputQuestion[],
  answers: Record<string, InputAnswer>,
): void {
  for (const question of questions) {
    if (!question.secret) continue;
    const entry = answers[question.id];
    if (entry) {
      entry.answers.fill("");
      entry.answers.length = 0;
    }
    delete answers[question.id];
  }
}
