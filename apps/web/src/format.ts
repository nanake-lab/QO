import type { Id, SessionOpenParams, TaskCreateParams } from "@qo/protocol";
import { zhCN } from "./i18n/zh-CN.js";
import { isQoRpcError } from "./protocol.js";

export type AssistantBlock =
  | { readonly kind: "text"; readonly text: string }
  | { readonly kind: "code"; readonly language: string; readonly code: string };

export type InlineSegment =
  | { readonly kind: "text"; readonly text: string }
  | { readonly kind: "code"; readonly text: string };

/**
 * 轻量的无依赖助手消息解析器：把 ``` 代码块拆分出来，
 * 流式输出尚未闭合的代码围栏也平滑渲染为代码块，不引入第三方 Markdown 库。
 */
export function parseAssistantBlocks(content: string): AssistantBlock[] {
  if (!content) return [];
  const normalized = content.replace(/\r\n/g, "\n");
  const lines = normalized.split("\n");
  const blocks: AssistantBlock[] = [];
  let textLines: string[] = [];
  let codeLines: string[] = [];
  let inCode = false;
  let codeLang = "";

  const flushText = () => {
    if (textLines.length === 0) return;
    const joined = textLines.join("\n");
    if (joined.length > 0) {
      blocks.push({ kind: "text", text: joined });
    }
    textLines = [];
  };

  for (const line of lines) {
    if (!inCode) {
      const open = line.match(/^```([^\s`]*)\s*$/);
      if (open) {
        flushText();
        inCode = true;
        codeLang = open[1] ?? "";
        codeLines = [];
      } else {
        textLines.push(line);
      }
    } else if (/^```\s*$/.test(line)) {
      blocks.push({
        kind: "code",
        language: codeLang,
        code: codeLines.join("\n"),
      });
      inCode = false;
      codeLang = "";
      codeLines = [];
    } else {
      codeLines.push(line);
    }
  }

  if (inCode) {
    blocks.push({
      kind: "code",
      language: codeLang,
      code: codeLines.join("\n"),
    });
  } else {
    flushText();
  }

  return blocks;
}

export function parseInlineCode(text: string): InlineSegment[] {
  if (!text) return [];
  const segments: InlineSegment[] = [];
  const pattern = /`([^`\n]+)`/g;
  let lastIndex = 0;
  for (const match of text.matchAll(pattern)) {
    const start = match.index ?? 0;
    if (start > lastIndex) {
      segments.push({ kind: "text", text: text.slice(lastIndex, start) });
    }
    segments.push({ kind: "code", text: match[1] });
    lastIndex = start + match[0].length;
  }
  if (lastIndex < text.length) {
    segments.push({ kind: "text", text: text.slice(lastIndex) });
  }
  return segments;
}

export type DiffLineKind = "add" | "del" | "hunk" | "meta" | "context";

export interface DiffLine {
  readonly kind: DiffLineKind;
  readonly text: string;
}

export function parseDiffLines(diff: string): DiffLine[] {
  if (!diff) return [];
  const normalized = diff.replace(/\r\n/g, "\n");
  const rawLines = normalized.split("\n");
  if (rawLines.length > 0 && rawLines[rawLines.length - 1] === "") {
    rawLines.pop();
  }
  return rawLines.map((line) => {
    if (
      line.startsWith("+++") ||
      line.startsWith("---") ||
      line.startsWith("diff --git ") ||
      line.startsWith("index ")
    ) {
      return { kind: "meta", text: line };
    }
    if (line.startsWith("@@")) {
      return { kind: "hunk", text: line };
    }
    if (line.startsWith("+")) {
      return { kind: "add", text: line };
    }
    if (line.startsWith("-")) {
      return { kind: "del", text: line };
    }
    return { kind: "context", text: line };
  });
}

export interface TaskCreateDraft {
  readonly name: string;
  readonly group: string;
  readonly cwd: string;
  readonly forbiddenRootsText: string;
}

export function buildTaskCreateParams(
  draft: TaskCreateDraft,
  taskId: Id = crypto.randomUUID(),
): TaskCreateParams {
  const name = draft.name.trim();
  const group = draft.group.trim();
  const cwd = draft.cwd.trim();
  const forbiddenRoots = draft.forbiddenRootsText
    .split(/\r?\n/)
    .map((line) => line.trim())
    .filter((line) => line.length > 0);

  return {
    taskId,
    name,
    ...(group ? { group } : {}),
    ...(cwd ? { cwd } : {}),
    ...(forbiddenRoots.length > 0 ? { forbiddenRoots } : {}),
  };
}

export interface SessionOpenDraft {
  readonly model: string;
  readonly reasoningEffort: string;
  readonly serviceTier: string;
}

export function buildSessionOpenParams(
  taskId: Id,
  draft: SessionOpenDraft,
  sessionId: Id = crypto.randomUUID(),
): SessionOpenParams {
  const model = draft.model.trim();
  const reasoningEffort = draft.reasoningEffort.trim();
  const serviceTier = draft.serviceTier.trim();

  return {
    taskId,
    sessionId,
    ...(model ? { model } : {}),
    ...(reasoningEffort ? { reasoningEffort } : {}),
    ...(serviceTier ? { serviceTier } : {}),
  };
}

export function formatRpcError(error: unknown): string {
  if (isQoRpcError(error)) {
    switch (error.reason) {
      case "unsupported_version":
        return zhCN.errors.unsupportedVersion;
      case "initialize_required":
        return zhCN.errors.initializeRequired;
      case "forbidden":
        return zhCN.errors.forbidden;
      case "not_found":
        return zhCN.errors.notFound;
      case "conflict":
        return zhCN.errors.conflict;
      case "cursor_expired":
        return zhCN.errors.cursorExpired;
      case "digest_mismatch":
        return zhCN.errors.digestMismatch;
      case "approval_expired":
        return zhCN.errors.approvalExpired;
      case "request_withdrawn":
        return zhCN.errors.requestWithdrawn;
      case "method_not_found":
        return zhCN.errors.methodNotFound;
      case "invalid_params":
        return zhCN.errors.invalidParams;
      case "internal_error":
        return zhCN.errors.internalError;
      default:
        return zhCN.errors.fallback(error.message);
    }
  }
  if (error instanceof Error && error.message) {
    return error.message;
  }
  return zhCN.errors.unknown;
}
