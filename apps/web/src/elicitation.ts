import type { ApprovalRequest, Item, RawRequest } from "@qo/protocol";
import { zhCN } from "./i18n/zh-CN.js";

export interface CommandApprovalDetails {
  readonly command: string | null;
  readonly cwd: string | null;
  readonly reason: string | null;
}

export interface FileApprovalDetails {
  readonly path: string | null;
  readonly diff: string | null;
  readonly reason: string | null;
}

export interface PermissionsApprovalDetails {
  readonly items: readonly string[];
  readonly cwd: string | null;
  readonly reason: string | null;
}

export interface ElicitationOption {
  readonly value: string;
  readonly label: string;
}

export type ElicitationField =
  | {
      readonly kind: "string";
      readonly name: string;
      readonly label: string;
      readonly description?: string;
      readonly required: boolean;
      readonly defaultValue?: string;
      readonly minLength?: number;
      readonly maxLength?: number;
    }
  | {
      readonly kind: "number" | "integer";
      readonly name: string;
      readonly label: string;
      readonly description?: string;
      readonly required: boolean;
      readonly defaultValue?: number;
      readonly minimum?: number;
      readonly maximum?: number;
    }
  | {
      readonly kind: "boolean";
      readonly name: string;
      readonly label: string;
      readonly description?: string;
      readonly required: boolean;
      readonly defaultValue?: boolean;
    }
  | {
      readonly kind: "enum";
      readonly name: string;
      readonly label: string;
      readonly description?: string;
      readonly required: boolean;
      readonly defaultValue?: string;
      readonly options: readonly ElicitationOption[];
    };

export type ParsedElicitation =
  | {
      readonly supported: true;
      readonly message: string | null;
      readonly serverName: string | null;
      readonly fields: readonly ElicitationField[];
    }
  | {
      readonly supported: false;
      readonly reason: "url_mode" | "unsupported_schema";
      readonly message: string | null;
      readonly serverName: string | null;
      readonly url: string | null;
    };

export type ElicitationFormValues = Record<string, string | boolean>;

export interface ElicitationBuildResult {
  readonly ok: true;
  readonly answer: Record<string, unknown>;
}

export interface ElicitationBuildError {
  readonly ok: false;
  readonly fieldErrors: Readonly<Record<string, string>>;
}

function isPlainObject(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function asNonEmptyString(value: unknown): string | null {
  return typeof value === "string" && value.length > 0 ? value : null;
}

function asFiniteNumber(value: unknown): number | undefined {
  return typeof value === "number" && Number.isFinite(value)
    ? value
    : undefined;
}

export function extractCommandApprovalDetails(
  request: ApprovalRequest,
  linkedItem?: Item | null,
): CommandApprovalDetails {
  const { params, context } = request.raw;
  const linkedCommand =
    linkedItem?.kind === "command_execution"
      ? linkedItem.content.command
      : null;

  const command =
    asNonEmptyString(params.command) ??
    asNonEmptyString(context?.command) ??
    linkedCommand;
  const cwd =
    asNonEmptyString(params.cwd) ?? asNonEmptyString(context?.cwd) ?? null;
  const reason =
    asNonEmptyString(params.reason) ?? asNonEmptyString(context?.reason);

  return { command, cwd, reason };
}

export function extractFileApprovalDetails(
  request: ApprovalRequest,
  linkedItem?: Item | null,
): FileApprovalDetails {
  const { params, context } = request.raw;
  const linkedFile =
    linkedItem?.kind === "file_change" ? linkedItem.content : null;

  const path =
    asNonEmptyString(params.path) ??
    asNonEmptyString(context?.path) ??
    linkedFile?.path ??
    null;
  const diff =
    (typeof params.diff === "string" ? params.diff : null) ??
    (typeof context?.diff === "string" ? context.diff : null) ??
    linkedFile?.diff ??
    null;
  const reason =
    asNonEmptyString(params.reason) ?? asNonEmptyString(context?.reason);

  return { path, diff, reason };
}

function formatFsAccess(mode: unknown): string {
  switch (mode) {
    case "read":
      return zhCN.approval.permFsRead;
    case "write":
      return zhCN.approval.permFsWrite;
    case "deny":
      return zhCN.approval.permFsDeny;
    default:
      return zhCN.approval.permFsOther;
  }
}

function formatFsPath(pathSpec: unknown): string | null {
  if (typeof pathSpec === "string" && pathSpec.length > 0) return pathSpec;
  if (!isPlainObject(pathSpec)) return null;
  if (typeof pathSpec.path === "string") return pathSpec.path;
  if (typeof pathSpec.pattern === "string") return pathSpec.pattern;
  return null;
}

export function extractPermissionsApprovalDetails(
  request: ApprovalRequest,
): PermissionsApprovalDetails {
  const { params, context } = request.raw;
  const rawPermissions = params.permissions ?? context?.permissions;
  const items: string[] = [];

  if (Array.isArray(rawPermissions)) {
    for (const entry of rawPermissions) {
      if (typeof entry === "string" && entry.trim().length > 0) {
        items.push(entry.trim());
      } else if (entry !== null && entry !== undefined) {
        items.push(JSON.stringify(entry));
      }
    }
  } else if (isPlainObject(rawPermissions)) {
    const fs = rawPermissions.fileSystem;
    if (isPlainObject(fs)) {
      if (Array.isArray(fs.entries)) {
        for (const entry of fs.entries) {
          if (!isPlainObject(entry)) continue;
          const target = formatFsPath(entry.path);
          if (target) {
            items.push(`${formatFsAccess(entry.access)}：${target}`);
          }
        }
      }
      if (Array.isArray(fs.read)) {
        for (const target of fs.read) {
          if (typeof target === "string" && target.length > 0) {
            items.push(`${zhCN.approval.permFsRead}：${target}`);
          }
        }
      }
      if (Array.isArray(fs.write)) {
        for (const target of fs.write) {
          if (typeof target === "string" && target.length > 0) {
            items.push(`${zhCN.approval.permFsWrite}：${target}`);
          }
        }
      }
      if (typeof fs.globScanMaxDepth === "number") {
        items.push(zhCN.approval.permGlobDepth(fs.globScanMaxDepth));
      }
    }

    const network = rawPermissions.network;
    if (isPlainObject(network) && typeof network.enabled === "boolean") {
      items.push(
        network.enabled
          ? zhCN.approval.permNetworkEnabled
          : zhCN.approval.permNetworkDisabled,
      );
    }

    for (const [key, value] of Object.entries(rawPermissions)) {
      if (key === "fileSystem" || key === "network") continue;
      if (value === null || value === undefined) continue;
      items.push(
        `${key}：${typeof value === "string" ? value : JSON.stringify(value)}`,
      );
    }
  }

  const cwd =
    asNonEmptyString(params.cwd) ?? asNonEmptyString(context?.cwd) ?? null;
  const reason =
    asNonEmptyString(params.reason) ?? asNonEmptyString(context?.reason);

  return { items, cwd, reason };
}

function parseSingleField(
  name: string,
  schema: unknown,
  required: boolean,
): ElicitationField | null {
  if (!isPlainObject(schema)) return null;

  // 第一版仅支持平铺简单字段，拒绝嵌套、引用或组合关键字。
  if (
    "$ref" in schema ||
    "allOf" in schema ||
    "anyOf" in schema ||
    "not" in schema ||
    "properties" in schema ||
    "items" in schema
  ) {
    return null;
  }

  const label = asNonEmptyString(schema.title) ?? name;
  const description = asNonEmptyString(schema.description) ?? undefined;
  const rawType = schema.type;

  // 带 oneOf const/title 的单选枚举（MCP 2025-11-25 titled single-select enum）
  if ("oneOf" in schema) {
    if (
      (rawType !== undefined && rawType !== "string") ||
      !Array.isArray(schema.oneOf) ||
      schema.oneOf.length === 0
    ) {
      return null;
    }
    const options: ElicitationOption[] = [];
    for (const opt of schema.oneOf) {
      if (
        !isPlainObject(opt) ||
        typeof opt.const !== "string" ||
        typeof opt.title !== "string"
      ) {
        return null;
      }
      options.push({ value: opt.const, label: opt.title });
    }
    return {
      kind: "enum",
      name,
      label,
      description,
      required,
      defaultValue:
        typeof schema.default === "string" ? schema.default : undefined,
      options,
    };
  }

  // 标准 enum（支持可选 enumNames）
  if ("enum" in schema) {
    if (
      (rawType !== undefined && rawType !== "string") ||
      !Array.isArray(schema.enum) ||
      schema.enum.length === 0
    ) {
      return null;
    }
    const enumNames = Array.isArray(schema.enumNames) ? schema.enumNames : null;
    const options: ElicitationOption[] = [];
    for (let i = 0; i < schema.enum.length; i += 1) {
      const value = schema.enum[i];
      if (typeof value !== "string") return null;
      const nameOverride =
        enumNames && typeof enumNames[i] === "string"
          ? (enumNames[i] as string)
          : value;
      options.push({ value, label: nameOverride });
    }
    return {
      kind: "enum",
      name,
      label,
      description,
      required,
      defaultValue:
        typeof schema.default === "string" ? schema.default : undefined,
      options,
    };
  }

  if (typeof rawType !== "string") return null;

  switch (rawType) {
    case "string":
      return {
        kind: "string",
        name,
        label,
        description,
        required,
        defaultValue:
          typeof schema.default === "string" ? schema.default : undefined,
        minLength: asFiniteNumber(schema.minLength),
        maxLength: asFiniteNumber(schema.maxLength),
      };
    case "number":
    case "integer":
      return {
        kind: rawType,
        name,
        label,
        description,
        required,
        defaultValue: asFiniteNumber(schema.default),
        minimum: asFiniteNumber(schema.minimum),
        maximum: asFiniteNumber(schema.maximum),
      };
    case "boolean":
      return {
        kind: "boolean",
        name,
        label,
        description,
        required,
        defaultValue:
          typeof schema.default === "boolean" ? schema.default : undefined,
      };
    default:
      return null;
  }
}

export function parseElicitation(raw: RawRequest): ParsedElicitation {
  const params = raw.params;
  const context = raw.context;
  const mode = asNonEmptyString(params.mode) ?? asNonEmptyString(context?.mode);
  const message =
    asNonEmptyString(params.message) ?? asNonEmptyString(context?.message);
  const serverName =
    asNonEmptyString(params.serverName) ??
    asNonEmptyString(context?.serverName);
  const url = asNonEmptyString(params.url) ?? asNonEmptyString(context?.url);

  if (mode === "url" || (mode === null && url !== null)) {
    return {
      supported: false,
      reason: "url_mode",
      message,
      serverName,
      url,
    };
  }

  if (mode !== null && mode !== "form") {
    return {
      supported: false,
      reason: "unsupported_schema",
      message,
      serverName,
      url,
    };
  }

  const schema = params.requestedSchema ?? context?.requestedSchema;
  if (!isPlainObject(schema)) {
    return {
      supported: false,
      reason: "unsupported_schema",
      message,
      serverName,
      url,
    };
  }

  if (
    schema.type !== "object" ||
    !isPlainObject(schema.properties) ||
    "$ref" in schema ||
    "oneOf" in schema ||
    "anyOf" in schema ||
    "allOf" in schema ||
    "not" in schema
  ) {
    return {
      supported: false,
      reason: "unsupported_schema",
      message,
      serverName,
      url,
    };
  }

  const requiredSet = new Set<string>();
  if (Array.isArray(schema.required)) {
    for (const key of schema.required) {
      if (typeof key === "string") {
        requiredSet.add(key);
      }
    }
  }

  const entries = Object.entries(schema.properties);
  const fields: ElicitationField[] = [];
  for (const [key, propSchema] of entries) {
    const parsed = parseSingleField(key, propSchema, requiredSet.has(key));
    if (!parsed) {
      return {
        supported: false,
        reason: "unsupported_schema",
        message,
        serverName,
        url,
      };
    }
    fields.push(parsed);
  }

  return {
    supported: true,
    message,
    serverName,
    fields,
  };
}

export function createInitialElicitationValues(
  fields: readonly ElicitationField[],
): ElicitationFormValues {
  const values: ElicitationFormValues = {};
  for (const field of fields) {
    switch (field.kind) {
      case "boolean":
        values[field.name] = field.defaultValue ?? false;
        break;
      case "number":
      case "integer":
        values[field.name] =
          field.defaultValue !== undefined ? String(field.defaultValue) : "";
        break;
      case "enum":
      case "string":
        values[field.name] = field.defaultValue ?? "";
        break;
    }
  }
  return values;
}

export function buildElicitationAnswer(
  fields: readonly ElicitationField[],
  values: ElicitationFormValues,
): ElicitationBuildResult | ElicitationBuildError {
  const answer: Record<string, unknown> = {};
  const fieldErrors: Record<string, string> = {};

  for (const field of fields) {
    const rawValue = values[field.name];
    if (field.kind === "boolean") {
      answer[field.name] = rawValue === true;
      continue;
    }

    const text = typeof rawValue === "string" ? rawValue : "";
    if (field.kind === "string") {
      if (text.length === 0) {
        if (field.required) {
          fieldErrors[field.name] = zhCN.approval.elicitationRequired;
        }
        continue;
      }
      if (field.minLength !== undefined && text.length < field.minLength) {
        fieldErrors[field.name] = zhCN.approval.elicitationMinLength(
          field.minLength,
        );
        continue;
      }
      if (field.maxLength !== undefined && text.length > field.maxLength) {
        fieldErrors[field.name] = zhCN.approval.elicitationMaxLength(
          field.maxLength,
        );
        continue;
      }
      answer[field.name] = text;
      continue;
    }

    if (field.kind === "enum") {
      if (text.length === 0) {
        if (field.required) {
          fieldErrors[field.name] = zhCN.approval.elicitationRequired;
        }
        continue;
      }
      if (!field.options.some((opt) => opt.value === text)) {
        fieldErrors[field.name] = zhCN.approval.elicitationInvalidOption;
        continue;
      }
      answer[field.name] = text;
      continue;
    }

    // number / integer
    const trimmed = text.trim();
    if (trimmed.length === 0) {
      if (field.required) {
        fieldErrors[field.name] = zhCN.approval.elicitationRequired;
      }
      continue;
    }
    const parsed = Number(trimmed);
    if (!Number.isFinite(parsed)) {
      fieldErrors[field.name] = zhCN.approval.elicitationInvalidNumber;
      continue;
    }
    if (field.kind === "integer" && !Number.isInteger(parsed)) {
      fieldErrors[field.name] = zhCN.approval.elicitationInvalidInteger;
      continue;
    }
    if (field.minimum !== undefined && parsed < field.minimum) {
      fieldErrors[field.name] = zhCN.approval.elicitationMinValue(
        field.minimum,
      );
      continue;
    }
    if (field.maximum !== undefined && parsed > field.maximum) {
      fieldErrors[field.name] = zhCN.approval.elicitationMaxValue(
        field.maximum,
      );
      continue;
    }
    answer[field.name] = parsed;
  }

  if (Object.keys(fieldErrors).length > 0) {
    return { ok: false, fieldErrors };
  }
  return { ok: true, answer };
}
