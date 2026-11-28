import { describe, expect, it } from "vitest";
import {
  buildSessionOpenParams,
  buildTaskCreateParams,
  formatRpcError,
  parseAssistantBlocks,
  parseDiffLines,
  parseInlineCode,
} from "./format.js";
import { zhCN } from "./i18n/zh-CN.js";
import { QoRpcError } from "./protocol.js";

const TASK_ID = "11111111-1111-4111-8111-111111111111";
const SESSION_ID = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa";

describe("assistant text and diff formatters", () => {
  it("parses plain text, closed fenced code blocks, and unclosed streaming fences", () => {
    const content = [
      "先检查配置：",
      "```ts",
      "const x = 1;",
      "```",
      "再执行测试。",
      "```sh",
      "pnpm test",
    ].join("\n");

    expect(parseAssistantBlocks(content)).toEqual([
      { kind: "text", text: "先检查配置：" },
      { kind: "code", language: "ts", code: "const x = 1;" },
      { kind: "text", text: "再执行测试。" },
      { kind: "code", language: "sh", code: "pnpm test" },
    ]);
    expect(parseAssistantBlocks("")).toEqual([]);
  });

  it("splits inline code segments without interpreting HTML", () => {
    expect(parseInlineCode("运行 `pnpm test` 和 `<script>`")).toEqual([
      { kind: "text", text: "运行 " },
      { kind: "code", text: "pnpm test" },
      { kind: "text", text: " 和 " },
      { kind: "code", text: "<script>" },
    ]);
  });

  it("classifies diff lines into meta, hunk, add, del, and context", () => {
    const diff = [
      "--- a/demo.txt",
      "+++ b/demo.txt",
      "@@ -1,2 +1,2 @@",
      " keep",
      "-old line",
      "+new line",
      "",
    ].join("\n");

    expect(parseDiffLines(diff)).toEqual([
      { kind: "meta", text: "--- a/demo.txt" },
      { kind: "meta", text: "+++ b/demo.txt" },
      { kind: "hunk", text: "@@ -1,2 +1,2 @@" },
      { kind: "context", text: " keep" },
      { kind: "del", text: "-old line" },
      { kind: "add", text: "+new line" },
    ]);
  });
});

describe("task and session payload builders", () => {
  it("omits empty optional task fields and parses multiline forbiddenRoots", () => {
    expect(
      buildTaskCreateParams(
        {
          name: "  示例任务  ",
          group: "   ",
          cwd: "",
          forbiddenRootsText: "\n  /etc \n\n /var/secrets  \n",
        },
        TASK_ID,
      ),
    ).toEqual({
      taskId: TASK_ID,
      name: "示例任务",
      forbiddenRoots: ["/etc", "/var/secrets"],
    });

    expect(
      buildTaskCreateParams(
        {
          name: "带目录",
          group: "核心",
          cwd: " /work/app ",
          forbiddenRootsText: "   ",
        },
        TASK_ID,
      ),
    ).toEqual({
      taskId: TASK_ID,
      name: "带目录",
      group: "核心",
      cwd: "/work/app",
    });
  });

  it("omits empty optional session.open fields", () => {
    expect(
      buildSessionOpenParams(
        TASK_ID,
        { model: "", reasoningEffort: "  ", serviceTier: "" },
        SESSION_ID,
      ),
    ).toEqual({
      taskId: TASK_ID,
      sessionId: SESSION_ID,
    });

    expect(
      buildSessionOpenParams(
        TASK_ID,
        { model: " mock ", reasoningEffort: "high", serviceTier: "fast" },
        SESSION_ID,
      ),
    ).toEqual({
      taskId: TASK_ID,
      sessionId: SESSION_ID,
      model: "mock",
      reasoningEffort: "high",
      serviceTier: "fast",
    });
  });

  it("formats all protocol RPC error reasons into plain Chinese", () => {
    const expectedByReason = [
      ["unsupported_version", zhCN.errors.unsupportedVersion],
      ["initialize_required", zhCN.errors.initializeRequired],
      ["forbidden", zhCN.errors.forbidden],
      ["not_found", zhCN.errors.notFound],
      ["conflict", zhCN.errors.conflict],
      ["cursor_expired", zhCN.errors.cursorExpired],
      ["digest_mismatch", zhCN.errors.digestMismatch],
      ["approval_expired", zhCN.errors.approvalExpired],
      ["request_withdrawn", zhCN.errors.requestWithdrawn],
      ["method_not_found", zhCN.errors.methodNotFound],
      ["invalid_params", zhCN.errors.invalidParams],
      ["internal_error", zhCN.errors.internalError],
    ] as const;

    for (const [reason, expected] of expectedByReason) {
      expect(
        formatRpcError(
          new QoRpcError({
            code: -32000,
            message: reason,
            data: { reason },
          }),
        ),
      ).toBe(expected);
    }

    expect(formatRpcError(new Error("自定义失败"))).toBe("自定义失败");
    expect(formatRpcError("raw")).toBe(zhCN.errors.unknown);
  });
});
