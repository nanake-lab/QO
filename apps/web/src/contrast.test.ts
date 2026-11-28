import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";

/** WCAG 2.1 AA 正文最低对比度要求。 */
const WCAG_AA_NORMAL_TEXT_RATIO = 4.5;

function parseCssHexTokens(css: string): Record<string, string> {
  const tokens: Record<string, string> = {};
  const pattern = /(--[a-z0-9-]+)\s*:\s*(#[0-9a-fA-F]{6})\s*;/g;
  for (const match of css.matchAll(pattern)) {
    tokens[match[1]] = match[2].toLowerCase();
  }
  return tokens;
}

function channelLuminance(hexPair: string): number {
  const srgb = Number.parseInt(hexPair, 16) / 255;
  return srgb <= 0.04045 ? srgb / 12.92 : ((srgb + 0.055) / 1.055) ** 2.4;
}

function relativeLuminance(hex: string): number {
  const r = channelLuminance(hex.slice(1, 3));
  const g = channelLuminance(hex.slice(3, 5));
  const b = channelLuminance(hex.slice(5, 7));
  return 0.2126 * r + 0.7152 * g + 0.0722 * b;
}

function contrastRatio(fgHex: string, bgHex: string): number {
  const l1 = relativeLuminance(fgHex);
  const l2 = relativeLuminance(bgHex);
  const lighter = Math.max(l1, l2);
  const darker = Math.min(l1, l2);
  return (lighter + 0.05) / (darker + 0.05);
}

describe("theme token WCAG AA contrast", () => {
  const css = readFileSync(new URL("./styles/tokens.css", import.meta.url), {
    encoding: "utf8",
  });
  const tokens = parseCssHexTokens(css);

  const surfaces = [
    "--bg-sidebar",
    "--bg-page",
    "--bg-surface",
    "--bg-surface-hover",
    "--bg-surface-active",
    "--diff-add-bg",
    "--diff-del-bg",
    "--diff-hunk-bg",
  ] as const;

  const textTokens = [
    "--text-primary",
    "--text-secondary",
    "--text-muted",
  ] as const;

  it("ensures every text token reaches WCAG AA (>= 4.5:1) on every surface", () => {
    for (const fg of textTokens) {
      for (const bg of surfaces) {
        expect(tokens[fg], `missing token ${fg}`).toBeDefined();
        expect(tokens[bg], `missing token ${bg}`).toBeDefined();
        const ratio = contrastRatio(tokens[fg], tokens[bg]);
        expect(
          ratio,
          `${fg} (${tokens[fg]}) on ${bg} (${tokens[bg]}) = ${ratio.toFixed(2)}:1`,
        ).toBeGreaterThanOrEqual(WCAG_AA_NORMAL_TEXT_RATIO);
      }
    }
  });

  it("ensures action accent and button text reach WCAG AA (>= 4.5:1)", () => {
    const actionSurfaces = [
      "--bg-sidebar",
      "--bg-page",
      "--bg-surface",
      "--bg-surface-hover",
      "--bg-surface-active",
    ] as const;

    for (const bg of actionSurfaces) {
      const ratio = contrastRatio(tokens["--color-action"], tokens[bg]);
      expect(
        ratio,
        `--color-action on ${bg} = ${ratio.toFixed(2)}:1`,
      ).toBeGreaterThanOrEqual(WCAG_AA_NORMAL_TEXT_RATIO);
    }

    const buttonRatio = contrastRatio(
      tokens["--color-action-fg"],
      tokens["--color-action"],
    );
    expect(
      buttonRatio,
      `--color-action-fg on --color-action = ${buttonRatio.toFixed(2)}:1`,
    ).toBeGreaterThanOrEqual(WCAG_AA_NORMAL_TEXT_RATIO);
  });
});
