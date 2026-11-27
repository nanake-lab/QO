import { describe, expect, it } from "vitest";
import manifest from "../package.json" with { type: "json" };
import web from "../apps/web/package.json" with { type: "json" };
import harnesses from "../config/harness.lock.json" with { type: "json" };

describe("workspace toolchain", () => {
  it("pins the Codex generator to the version used by the stdio adapter", () => {
    expect(harnesses.codex).toEqual({
      version: manifest.devDependencies["@openai/codex"],
      transport: "stdio",
      protocol: "app-server",
    });
  });

  it("uses exact frontend and generator releases", () => {
    expect(manifest.packageManager).toBe("pnpm@12.11.2");
    expect(manifest.engines.node).toBe("24.x");
    const { "@qo/protocol": local, ...runtime } = web.dependencies;
    expect(local).toBe("workspace:*");
    for (const version of Object.values({
      ...manifest.devDependencies,
      ...web.devDependencies,
      ...runtime,
    })) {
      expect(version).toMatch(/^\d+\.\d+\.\d+$/);
    }
  });
});
