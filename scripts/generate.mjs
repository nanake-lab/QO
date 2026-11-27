import { execFileSync } from "node:child_process";
import {
  readFileSync,
  mkdirSync,
  mkdtempSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { JSONSchemaInput, InputData, quicktype } from "quicktype-core";

import { format } from "prettier";
import { publicTypes, methodTable } from "./protocol.mjs";

const check = process.argv.includes("--check");
const read = (file) => readFileSync(file, "utf8").replaceAll("\r\n", "\n");
function emit(file, contents) {
  if (check) {
    if (read(file) !== contents)
      throw new Error("Stale generated file: " + file);
  } else {
    mkdirSync(dirname(file), { recursive: true });
    writeFileSync(file, contents);
  }
}

async function render(lang, schemas) {
  const input = new JSONSchemaInput(undefined);
  for (const schema of schemas) {
    await input.addSource({
      name: schema.title,
      schema: JSON.stringify(schema),
    });
  }
  const data = new InputData();
  data.addInput(input);
  const options =
    lang === "rust" ? { visibility: "public" } : { "just-types": "true" };
  const result = await quicktype({
    inputData: data,
    lang,
    rendererOptions: options,
    leadingComments: [
      "Generated from JSON Schema. Run pnpm generate to update.",
    ],
  });
  const code = result.lines.join("\n").trimEnd() + "\n";
  return lang === "rust"
    ? execFileSync("rustfmt", ["--edition", "2024", "--emit", "stdout"], {
        input: code,
        encoding: "utf8",
      }).replaceAll("\r\n", "\n")
    : code;
}

const dir = "packages/protocol/schema";
const { definitions } = JSON.parse(read(join(dir, "protocol.json")));
const catalog = JSON.parse(read(join(dir, "methods.json")));
function rustfmt(code) {
  return execFileSync("rustfmt", ["--edition", "2024", "--emit", "stdout"], {
    input: code,
    encoding: "utf8",
  }).replaceAll("\r\n", "\n");
}
emit(
  "packages/protocol/generated/types.ts",
  await format(publicTypes(definitions, "ts"), { parser: "typescript" }),
);
emit(
  "packages/protocol/generated/types.rs",
  rustfmt(publicTypes(definitions, "rs")),
);
emit(
  "packages/protocol/generated/methods.rs",
  rustfmt(methodTable(catalog, definitions)),
);

// Use the workspace-pinned executable even when the operator has another Codex on PATH.
const lock = JSON.parse(read("config/harness.lock.json")).codex;
const codex = resolve("node_modules/@openai/codex/bin/codex.js");
const runCodex = (args) =>
  execFileSync(process.execPath, [codex, ...args], { encoding: "utf8" }).trim();
if (runCodex(["--version"]) !== "codex-cli " + lock.version)
  throw new Error("Codex version mismatch");
const tmp = mkdtempSync(join(tmpdir(), "qo-codex-schema-"));
try {
  runCodex(["app-server", "generate-json-schema", "--out", tmp]);
  const schema = JSON.parse(read(join(tmp, "ServerRequest.json")));
  emit(
    "packages/adapter-codex/schema/ServerRequest.json",
    JSON.stringify(schema, null, 2) + "\n",
  );
  // Keep the wire discriminator even where several callbacks have overlapping payload shapes.
  const variants = schema.oneOf.map(({ properties }) => {
    const [method] = properties.method.enum;
    const type = properties.params.$ref.split("/").at(-1);
    if (properties.method.enum.length !== 1 || !schema.definitions[type])
      throw new Error("Unsupported callback schema: " + method);
    return { method, type, variant: type.replace(/Params$/, "") };
  });
  const methods = variants
    .map(
      ({ method, variant }) =>
        '    #[serde(rename = "' + method + '")]\n    ' + variant + ",",
    )
    .join("\n");
  emit(
    "packages/adapter-codex/src/method.rs",
    "// Generated from the pinned Codex ServerRequest schema.\n#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize)]\npub enum Method {\n" +
      methods +
      "\n}\n",
  );
  // Export the payloads as named roots so callers can decode after matching Method.
  const payloads = variants.map(({ type }) => ({
    title: type,
    ...schema.definitions[type],
    definitions: schema.definitions,
  }));
  emit(
    "packages/adapter-codex/src/generated.rs",
    await render("rust", payloads),
  );
} finally {
  rmSync(tmp, { recursive: true, force: true });
}
