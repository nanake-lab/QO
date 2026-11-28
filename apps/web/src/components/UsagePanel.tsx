import type { Turn } from "@qo/protocol";
import { zhCN } from "../i18n/zh-CN.js";
import { buildUsageMetrics, type UsageSourceGroup } from "../usage.js";

interface UsagePanelProps {
  readonly groups: readonly UsageSourceGroup[];
  readonly turns: readonly Turn[];
}

export function UsagePanel({ groups, turns }: UsagePanelProps) {
  const turnIndexById = new Map<string, number>();
  turns.forEach((turn, idx) => {
    turnIndexById.set(turn.turnId, idx + 1);
  });

  return (
    <section className="usage-panel" aria-label={zhCN.usage.heading}>
      <div className="usage-panel-header">
        <h3 className="usage-panel-heading">{zhCN.usage.heading}</h3>
        {groups.length > 1 ? (
          <span className="muted-hint">{zhCN.usage.separateSourcesNote}</span>
        ) : null}
      </div>

      {groups.length === 0 ? (
        <p className="muted-hint">{zhCN.usage.empty}</p>
      ) : (
        <div className="usage-source-list">
          {groups.map((group) => (
            <div key={group.source} className="usage-source-block">
              <div className="usage-source-header">
                <span className="meta-field-label">
                  {zhCN.usage.sourceLabel}
                </span>
                <code className="usage-source-name mono">{group.source}</code>
              </div>

              <div className="usage-entries">
                {group.entries.map((entry, idx) => {
                  const turnNumber = entry.turnId
                    ? turnIndexById.get(entry.turnId)
                    : undefined;
                  const metrics = buildUsageMetrics(entry.counts);
                  const entryKey = `${entry.source}:${entry.turnId ?? ""}:${entry.model ?? ""}:${entry.reasoningEffort ?? ""}:${entry.serviceTier ?? ""}:${idx}`;
                  return (
                    <div key={entryKey} className="usage-entry">
                      <div className="usage-dimensions">
                        {turnNumber !== undefined ? (
                          <span className="usage-dim tabular">
                            {zhCN.usage.turnIndex(turnNumber)}
                          </span>
                        ) : null}
                        <span className="usage-dim">
                          <span className="usage-dim-label">
                            {zhCN.usage.modelLabel}
                          </span>{" "}
                          <span className="mono">
                            {entry.model ?? zhCN.overview.unreportedValue}
                          </span>
                        </span>
                        <span className="usage-dim">
                          <span className="usage-dim-label">
                            {zhCN.usage.reasoningEffortLabel}
                          </span>{" "}
                          <span>
                            {entry.reasoningEffort ??
                              zhCN.overview.unreportedValue}
                          </span>
                        </span>
                        <span className="usage-dim">
                          <span className="usage-dim-label">
                            {zhCN.usage.serviceTierLabel}
                          </span>{" "}
                          <span>
                            {entry.serviceTier ?? zhCN.overview.unreportedValue}
                          </span>
                        </span>
                      </div>

                      <div className="usage-counts-line tabular">
                        {metrics.map((metric, mIdx) => (
                          <span key={metric.key} className="usage-metric">
                            {mIdx > 0 ? (
                              <span
                                className="usage-metric-sep"
                                aria-hidden="true"
                              >
                                ·
                              </span>
                            ) : null}
                            <span className="usage-metric-label">
                              {metric.label}
                            </span>{" "}
                            <span
                              className={
                                metric.formatted.missing
                                  ? "usage-metric-missing"
                                  : "usage-metric-value"
                              }
                            >
                              {metric.formatted.text}
                            </span>
                          </span>
                        ))}
                      </div>
                    </div>
                  );
                })}
              </div>
            </div>
          ))}
        </div>
      )}
    </section>
  );
}
