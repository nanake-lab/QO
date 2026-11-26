import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";
import manifest from "../package.json" with { type: "json" };
import harnesses from "../config/harness.lock.json" with { type: "json" };

describe("workspace bootstrap", () => {
  it.each(["protocol", "server", "adapter-codex"])(
    "recognizes the private %s package",
    (name) => {
      const url = new URL(`../packages/${name}/package.json`, import.meta.url);
      expect(JSON.parse(readFileSync(url, "utf8"))).toMatchObject({
        name: `@qo/${name}`,
        private: true,
        type: "module",
      });
    },
  );

  it("pins the toolchain and the Codex stdio harness", () => {
    const release = /^\d+\.\d+\.\d+$/;
    expect(manifest.packageManager).toBe("pnpm@12.11.2");
    expect(manifest.engines.node).toBe("24.x");
    for (const version of Object.values(manifest.devDependencies)) {
      expect(version).toMatch(release);
    }
    expect(harnesses.codex).toEqual({
      version: expect.stringMatching(release),
      transport: "stdio",
      protocol: "app-server",
    });
  });
});
