import { describe, expect, it } from "vitest";
import type { Usage } from "@qo/protocol";
import { zhCN } from "./i18n/zh-CN.js";
import { createInitialState, usageKey, type AppState } from "./state.js";
import {
  buildUsageMetrics,
  formatDecimalWithCommas,
  formatUsageCount,
  groupSessionUsage,
} from "./usage.js";

const SESSION_A = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa";
const SESSION_B = "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb";

function makeUsage(overrides: Partial<Usage>): Usage {
  return {
    sessionId: SESSION_A,
    source: "mock/turn",
    counts: {
      inputTokens: "12345",
      outputTokens: "2345",
      cachedInputTokens: "10000",
      cacheWriteInputTokens: null,
      reasoningOutputTokens: "0",
      totalTokens: "14690",
    },
    observedAt: "2026-11-28T10:00:00Z",
    ...overrides,
  };
}

describe("usage formatting and grouping", () => {
  it("adds thousands separators purely on decimal strings without losing precision", () => {
    expect(formatDecimalWithCommas("0")).toBe("0");
    expect(formatDecimalWithCommas("8")).toBe("8");
    expect(formatDecimalWithCommas("999")).toBe("999");
    expect(formatDecimalWithCommas("1000")).toBe("1,000");
    expect(formatDecimalWithCommas("12345")).toBe("12,345");
    // 超过 Number.MAX_SAFE_INTEGER (9007199254740991) 仍完整保留末位数字。
    expect(formatDecimalWithCommas("9007199254740993123456789")).toBe(
      "9,007,199,254,740,993,123,456,789",
    );
  });

  it("distinguishes null (拿不到) from string '0'", () => {
    expect(formatUsageCount(null)).toEqual({
      text: zhCN.usage.unavailable,
      missing: true,
    });
    expect(formatUsageCount("0")).toEqual({
      text: "0",
      missing: false,
    });
    expect(formatUsageCount("10000")).toEqual({
      text: "10,000",
      missing: false,
    });
  });

  it("builds all six usage metrics in protocol display order", () => {
    const metrics = buildUsageMetrics({
      inputTokens: "12345",
      outputTokens: "2345",
      cachedInputTokens: "10000",
      cacheWriteInputTokens: null,
      reasoningOutputTokens: "0",
      totalTokens: "14690",
    });

    expect(
      metrics.map((m) => [
        m.key,
        m.label,
        m.formatted.text,
        m.formatted.missing,
      ]),
    ).toEqual([
      ["inputTokens", zhCN.usage.inputTokens, "12,345", false],
      ["outputTokens", zhCN.usage.outputTokens, "2,345", false],
      ["cachedInputTokens", zhCN.usage.cachedInputTokens, "10,000", false],
      [
        "cacheWriteInputTokens",
        zhCN.usage.cacheWriteInputTokens,
        "拿不到",
        true,
      ],
      ["reasoningOutputTokens", zhCN.usage.reasoningOutputTokens, "0", false],
      ["totalTokens", zhCN.usage.totalTokens, "14,690", false],
    ]);
  });

  it("keeps distinct usage sources separate and never sums them", () => {
    const u1 = makeUsage({
      source: "mock/turn",
      model: "gpt-5",
      reasoningEffort: "high",
      serviceTier: "fast",
      counts: {
        inputTokens: "100",
        outputTokens: "50",
        cachedInputTokens: null,
        cacheWriteInputTokens: null,
        reasoningOutputTokens: null,
        totalTokens: "150",
      },
      observedAt: "2026-11-28T10:00:00Z",
    });
    const u2 = makeUsage({
      source: "codex/session",
      model: "gpt-5",
      counts: {
        inputTokens: "200",
        outputTokens: "80",
        cachedInputTokens: "0",
        cacheWriteInputTokens: null,
        reasoningOutputTokens: null,
        totalTokens: "280",
      },
      observedAt: "2026-11-28T10:01:00Z",
    });
    const otherSession = makeUsage({
      sessionId: SESSION_B,
      source: "mock/turn",
    });

    const state: AppState = {
      ...createInitialState(),
      usage: {
        [usageKey(u1)]: u1,
        [usageKey(u2)]: u2,
        [usageKey(otherSession)]: otherSession,
      },
    };

    const groups = groupSessionUsage(state, SESSION_A);
    expect(groups.map((g) => g.source)).toEqual(["codex/session", "mock/turn"]);
    expect(groups[0].entries).toEqual([u2]);
    expect(groups[1].entries).toEqual([u1]);
    expect(groups[0].entries[0].counts.totalTokens).toBe("280");
    expect(groups[1].entries[0].counts.totalTokens).toBe("150");
  });
});
