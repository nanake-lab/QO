import { describe, expect, it } from "vitest";
import type { ApprovalRequest, RawRequest } from "@qo/protocol";
import {
  buildElicitationAnswer,
  createInitialElicitationValues,
  extractCommandApprovalDetails,
  extractFileApprovalDetails,
  extractPermissionsApprovalDetails,
  parseElicitation,
} from "./elicitation.js";

const BASE_APPROVAL: ApprovalRequest = {
  approvalId: "11111111-1111-4111-8111-111111111111",
  taskId: "22222222-2222-4222-8222-222222222222",
  sessionId: "33333333-3333-4333-8333-333333333333",
  kind: "elicitation",
  raw: {
    method: "mcpServer/elicitation/request",
    params: {},
  },
  summary: "请填写配置",
  digest: "a".repeat(64),
  expiresAt: "2026-11-28T10:00:00.000Z",
  state: "pending",
};

describe("elicitation schema conversion", () => {
  it("converts flat schemas with string, number, integer, boolean, enum, and titled oneOf enum", () => {
    const raw: RawRequest = {
      method: "mcpServer/elicitation/request",
      params: {
        serverName: "deploy-mcp",
        mode: "form",
        message: "确认本次发布参数",
        requestedSchema: {
          type: "object",
          required: ["tag", "replicas", "region"],
          properties: {
            tag: {
              type: "string",
              title: "版本标签",
              description: "发布版本号",
              minLength: 2,
              default: "v1.0",
            },
            ratio: {
              type: "number",
              title: "灰度比例",
              minimum: 0,
              maximum: 1,
              default: 0.25,
            },
            replicas: {
              type: "integer",
              title: "副本数",
              minimum: 1,
              maximum: 10,
            },
            dryRun: {
              type: "boolean",
              title: "仅预演",
              default: true,
            },
            region: {
              type: "string",
              title: "区域",
              enum: ["cn", "us"],
              enumNames: ["中国区", "美区"],
            },
            channel: {
              type: "string",
              title: "渠道",
              oneOf: [
                { const: "stable", title: "稳定版" },
                { const: "beta", title: "测试版" },
              ],
            },
          },
        },
      },
    };

    const parsed = parseElicitation(raw);
    expect(parsed.supported).toBe(true);
    if (!parsed.supported) return;

    expect(parsed.serverName).toBe("deploy-mcp");
    expect(parsed.message).toBe("确认本次发布参数");
    expect(parsed.fields).toEqual([
      {
        kind: "string",
        name: "tag",
        label: "版本标签",
        description: "发布版本号",
        required: true,
        defaultValue: "v1.0",
        minLength: 2,
        maxLength: undefined,
      },
      {
        kind: "number",
        name: "ratio",
        label: "灰度比例",
        description: undefined,
        required: false,
        defaultValue: 0.25,
        minimum: 0,
        maximum: 1,
      },
      {
        kind: "integer",
        name: "replicas",
        label: "副本数",
        description: undefined,
        required: true,
        defaultValue: undefined,
        minimum: 1,
        maximum: 10,
      },
      {
        kind: "boolean",
        name: "dryRun",
        label: "仅预演",
        description: undefined,
        required: false,
        defaultValue: true,
      },
      {
        kind: "enum",
        name: "region",
        label: "区域",
        description: undefined,
        required: true,
        defaultValue: undefined,
        options: [
          { value: "cn", label: "中国区" },
          { value: "us", label: "美区" },
        ],
      },
      {
        kind: "enum",
        name: "channel",
        label: "渠道",
        description: undefined,
        required: false,
        defaultValue: undefined,
        options: [
          { value: "stable", label: "稳定版" },
          { value: "beta", label: "测试版" },
        ],
      },
    ]);

    const defaults = createInitialElicitationValues(parsed.fields);
    expect(defaults).toEqual({
      tag: "v1.0",
      ratio: "0.25",
      replicas: "",
      dryRun: true,
      region: "",
      channel: "",
    });

    // 校验必填项与整数类型
    const invalid = buildElicitationAnswer(parsed.fields, {
      ...defaults,
      replicas: "2.5",
    });
    expect(invalid.ok).toBe(false);
    if (!invalid.ok) {
      expect(invalid.fieldErrors.replicas).toBeTruthy();
      expect(invalid.fieldErrors.region).toBeTruthy();
    }

    // 合法填写时组装出强类型 answer，未填的可选字段省略
    const valid = buildElicitationAnswer(parsed.fields, {
      ...defaults,
      replicas: "3",
      region: "cn",
      channel: "",
    });
    expect(valid).toEqual({
      ok: true,
      answer: {
        tag: "v1.0",
        ratio: 0.25,
        replicas: 3,
        dryRun: true,
        region: "cn",
      },
    });
  });

  it("marks URL mode, nested objects, array multi-select, and combinator schemas as unsupported", () => {
    const urlMode = parseElicitation({
      method: "mcpServer/elicitation/request",
      params: {
        serverName: "oauth-mcp",
        mode: "url",
        elicitationId: "e-1",
        message: "请在浏览器完成授权",
        url: "https://example.invalid/auth",
      },
    });
    expect(urlMode).toEqual({
      supported: false,
      reason: "url_mode",
      message: "请在浏览器完成授权",
      serverName: "oauth-mcp",
      url: "https://example.invalid/auth",
    });

    const nestedObject = parseElicitation({
      method: "mcpServer/elicitation/request",
      params: {
        mode: "form",
        message: "嵌套对象",
        requestedSchema: {
          type: "object",
          properties: {
            meta: { type: "object", properties: { a: { type: "string" } } },
          },
        },
      },
    });
    expect(nestedObject.supported).toBe(false);

    const multiSelectArray = parseElicitation({
      method: "mcpServer/elicitation/request",
      params: {
        mode: "form",
        message: "多选数组",
        requestedSchema: {
          type: "object",
          properties: {
            tags: {
              type: "array",
              items: { type: "string", enum: ["a", "b"] },
            },
          },
        },
      },
    });
    expect(multiSelectArray.supported).toBe(false);

    const openaiForm = parseElicitation({
      method: "mcpServer/elicitation/request",
      params: {
        mode: "openai/form",
        message: "专有模式",
        requestedSchema: { type: "object", properties: {} },
      },
    });
    expect(openaiForm.supported).toBe(false);
  });
});

describe("approval detail extractors", () => {
  it("extracts command/cwd, file path/diff, and structured permission lists", () => {
    const cmd = extractCommandApprovalDetails({
      ...BASE_APPROVAL,
      kind: "command_execution",
      raw: {
        method: "item/commandExecution/requestApproval",
        params: { command: "pnpm test", cwd: "/repo/qo", reason: "运行测试" },
      },
    });
    expect(cmd).toEqual({
      command: "pnpm test",
      cwd: "/repo/qo",
      reason: "运行测试",
    });

    const file = extractFileApprovalDetails({
      ...BASE_APPROVAL,
      kind: "file_change",
      raw: {
        method: "item/fileChange/requestApproval",
        params: {},
        context: { path: "demo.txt", diff: "+hello\n" },
      },
    });
    expect(file).toEqual({
      path: "demo.txt",
      diff: "+hello\n",
      reason: null,
    });

    const perms = extractPermissionsApprovalDetails({
      ...BASE_APPROVAL,
      kind: "permissions",
      raw: {
        method: "item/permissions/requestApproval",
        params: {
          cwd: "/repo/qo",
          reason: "需要写入构建产物并访问网络",
          permissions: {
            fileSystem: {
              entries: [
                {
                  path: { type: "path", path: "/repo/qo/dist" },
                  access: "write",
                },
                {
                  path: { type: "glob_pattern", pattern: "/repo/qo/**/*.ts" },
                  access: "read",
                },
              ],
            },
            network: { enabled: true },
          },
        },
      },
    });
    expect(perms.cwd).toBe("/repo/qo");
    expect(perms.reason).toBe("需要写入构建产物并访问网络");
    expect(perms.items).toEqual([
      "写入文件：/repo/qo/dist",
      "读取文件：/repo/qo/**/*.ts",
      "网络访问：允许",
    ]);
  });
});
