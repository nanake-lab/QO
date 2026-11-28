import { Fragment, useState } from "react";
import type { Item, Turn } from "@qo/protocol";
import {
  parseAssistantBlocks,
  parseDiffLines,
  parseInlineCode,
} from "../format.js";
import { zhCN } from "../i18n/zh-CN.js";
import { getTurnItems, type AppState } from "../state.js";

interface ConversationStreamProps {
  readonly state: AppState;
  readonly turns: readonly Turn[];
}

function AssistantMessageView({ content }: { readonly content: string }) {
  const blocks = parseAssistantBlocks(content);
  if (blocks.length === 0) return null;

  return (
    <div className="assistant-body">
      {blocks.map((block, index) => {
        if (block.kind === "code") {
          return (
            <div key={index} className="code-block">
              <div className="code-block-header">
                <span className="block-label mono">
                  {block.language || zhCN.stream.codeBlockLabel}
                </span>
              </div>
              <pre className="code-pre mono">
                <code>{block.code}</code>
              </pre>
            </div>
          );
        }
        const segments = parseInlineCode(block.text);
        return (
          <div key={index} className="assistant-text">
            {segments.map((segment, segIndex) =>
              segment.kind === "code" ? (
                <code key={segIndex} className="inline-code mono">
                  {segment.text}
                </code>
              ) : (
                <Fragment key={segIndex}>{segment.text}</Fragment>
              ),
            )}
          </div>
        );
      })}
    </div>
  );
}

function CommandItemView({
  item,
}: {
  readonly item: Extract<Item, { kind: "command_execution" }>;
}) {
  const [collapsed, setCollapsed] = useState(false);
  const output = item.content.output;
  const hasOutput = output.length > 0;
  const lineCount = hasOutput
    ? output.replace(/\r\n/g, "\n").replace(/\n$/, "").split("\n").length
    : 0;

  return (
    <div className="item-block">
      <div className="item-header">
        <div className="item-header-left">
          <span className="block-label">{zhCN.stream.commandLabel}</span>
          <span className="item-state-tag">{zhCN.itemState[item.state]}</span>
          {item.content.exitCode !== undefined ? (
            <span
              className={
                item.content.exitCode === 0
                  ? "item-state-tag tabular"
                  : "item-state-tag item-state-failed tabular"
              }
            >
              {zhCN.stream.exitCodeLabel(item.content.exitCode)}
            </span>
          ) : null}
        </div>
        {hasOutput ? (
          <button
            type="button"
            className="btn-link tabular"
            aria-expanded={!collapsed}
            onClick={() => setCollapsed((prev) => !prev)}
          >
            {collapsed
              ? `${zhCN.stream.expandOutput}（${zhCN.stream.outputLineCount(lineCount)}）`
              : zhCN.stream.collapseOutput}
          </button>
        ) : null}
      </div>

      <div className="command-line mono">$ {item.content.command}</div>

      {hasOutput ? (
        collapsed ? null : (
          <pre className="item-pre mono">{output}</pre>
        )
      ) : (
        <p className="muted-hint">{zhCN.stream.emptyOutput}</p>
      )}
    </div>
  );
}

function FileChangeItemView({
  item,
}: {
  readonly item: Extract<Item, { kind: "file_change" }>;
}) {
  const lines = parseDiffLines(item.content.diff);

  return (
    <div className="item-block">
      <div className="item-header">
        <div className="item-header-left">
          <span className="block-label">{zhCN.stream.fileChangeLabel}</span>
          <code className="file-path mono">{item.content.path}</code>
        </div>
        <span className="item-state-tag">{zhCN.itemState[item.state]}</span>
      </div>

      {lines.length === 0 ? (
        <p className="muted-hint">{zhCN.stream.emptyDiff}</p>
      ) : (
        <div
          className="diff-view mono"
          role="region"
          aria-label={item.content.path}
        >
          {lines.map((line, index) => (
            <div key={index} className={`diff-line diff-line-${line.kind}`}>
              {line.text || " "}
            </div>
          ))}
        </div>
      )}
    </div>
  );
}

export function ConversationStream({ state, turns }: ConversationStreamProps) {
  return (
    <section className="turns-section" aria-label={zhCN.stream.turnsHeading}>
      <h3 className="turns-heading">{zhCN.stream.turnsHeading}</h3>
      {turns.length === 0 ? (
        <p className="empty-state-body">{zhCN.stream.noTurnsBody}</p>
      ) : (
        turns.map((turn) => {
          const items = getTurnItems(state, turn.turnId);
          return (
            <div key={turn.turnId} className="turn-card">
              <div className="turn-header">
                <span className="block-label">
                  {zhCN.stream.userInputLabel}
                </span>
                <span
                  className={
                    turn.state === "failed"
                      ? "item-state-tag item-state-failed"
                      : "item-state-tag"
                  }
                >
                  {zhCN.turnState[turn.state]}
                </span>
              </div>

              <p className="turn-input-text">{turn.input}</p>

              {items.map((item) => {
                if (item.kind === "assistant_message") {
                  return (
                    <div key={item.itemId} className="item-block">
                      <div className="item-header">
                        <span className="block-label">
                          {zhCN.stream.assistantLabel}
                        </span>
                        <span className="item-state-tag">
                          {zhCN.itemState[item.state]}
                        </span>
                      </div>
                      <AssistantMessageView content={item.content} />
                    </div>
                  );
                }
                if (item.kind === "command_execution") {
                  return <CommandItemView key={item.itemId} item={item} />;
                }
                return <FileChangeItemView key={item.itemId} item={item} />;
              })}
            </div>
          );
        })
      )}
    </section>
  );
}
