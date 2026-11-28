import type { Id, Usage, UsageCount, UsageCounts } from "@qo/protocol";
import { zhCN } from "./i18n/zh-CN.js";
import type { AppState } from "./state.js";

export interface FormattedUsageCount {
  readonly text: string;
  readonly missing: boolean;
}

export interface UsageMetricItem {
  readonly key: keyof UsageCounts;
  readonly label: string;
  readonly formatted: FormattedUsageCount;
}

export interface UsageSourceGroup {
  readonly source: string;
  readonly entries: readonly Usage[];
}

/**
 * 纯字符串按千分位插入逗号，避免转成 JS Number 丢失超出 2^53-1 的大整数精度。
 */
export function formatDecimalWithCommas(value: string): string {
  const trimmed = value.trim();
  if (!/^\d+$/.test(trimmed)) {
    return value;
  }
  return trimmed.replace(/\B(?=(\d{3})+(?!\d))/g, ",");
}

export function formatUsageCount(count: UsageCount): FormattedUsageCount {
  if (count === null) {
    return { text: zhCN.usage.unavailable, missing: true };
  }
  return { text: formatDecimalWithCommas(count), missing: false };
}

export function buildUsageMetrics(counts: UsageCounts): UsageMetricItem[] {
  return [
    {
      key: "inputTokens",
      label: zhCN.usage.inputTokens,
      formatted: formatUsageCount(counts.inputTokens),
    },
    {
      key: "outputTokens",
      label: zhCN.usage.outputTokens,
      formatted: formatUsageCount(counts.outputTokens),
    },
    {
      key: "cachedInputTokens",
      label: zhCN.usage.cachedInputTokens,
      formatted: formatUsageCount(counts.cachedInputTokens),
    },
    {
      key: "cacheWriteInputTokens",
      label: zhCN.usage.cacheWriteInputTokens,
      formatted: formatUsageCount(counts.cacheWriteInputTokens),
    },
    {
      key: "reasoningOutputTokens",
      label: zhCN.usage.reasoningOutputTokens,
      formatted: formatUsageCount(counts.reasoningOutputTokens),
    },
    {
      key: "totalTokens",
      label: zhCN.usage.totalTokens,
      formatted: formatUsageCount(counts.totalTokens),
    },
  ];
}

/**
 * 按 ADR 第 9 节：不同来源分开分组，绝不跨来源相加。
 */
export function groupSessionUsage(
  state: AppState,
  sessionId: Id,
): UsageSourceGroup[] {
  const bySource = new Map<string, Usage[]>();
  for (const entry of Object.values(state.usage)) {
    if (entry.sessionId !== sessionId) continue;
    const list = bySource.get(entry.source) ?? [];
    list.push(entry);
    bySource.set(entry.source, list);
  }

  return [...bySource.keys()]
    .sort((a, b) => a.localeCompare(b))
    .map((source) => {
      const entries = bySource.get(source) ?? [];
      entries.sort((a, b) => {
        if (a.observedAt !== b.observedAt) {
          return a.observedAt < b.observedAt ? -1 : 1;
        }
        return JSON.stringify([
          a.turnId ?? "",
          a.model ?? "",
          a.reasoningEffort ?? "",
          a.serviceTier ?? "",
        ]).localeCompare(
          JSON.stringify([
            b.turnId ?? "",
            b.model ?? "",
            b.reasoningEffort ?? "",
            b.serviceTier ?? "",
          ]),
        );
      });
      return { source, entries };
    });
}
