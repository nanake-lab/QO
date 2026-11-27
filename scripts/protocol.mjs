// Render the QO wire shapes. Keep named aliases and discriminants even when
// several methods have identical fields. Formats and state constraints are
// checked against the original draft-07 document by the Rust validator.
const variantsOf = (schema) => schema.oneOf?.map((branch) => branch.properties);
const variantName = (name) =>
  name
    .split(/[^A-Za-z0-9]+/)
    .map((word) => word[0].toUpperCase() + word.slice(1))
    .join("");
const snake = (name) => name.replace(/[A-Z]/g, (c) => "_" + c.toLowerCase());
const quote = JSON.stringify;
const banner =
  "// Generated from the QO protocol schema. Run pnpm generate to update.\n";

function wireType(schema, lang) {
  if (schema.$ref) return schema.$ref.slice("#/definitions/".length);
  if (schema.const !== undefined && lang === "ts") return quote(schema.const);
  if (Array.isArray(schema.type) && schema.type.includes("null")) {
    const inner = wireType(
      { ...schema, type: schema.type.find((t) => t !== "null") },
      lang,
    );
    return lang === "ts" ? inner + " | null" : `Option<${inner}>`;
  }
  switch (schema.type) {
    case "boolean":
      return lang === "ts" ? "boolean" : "bool";
    case "integer":
      return lang === "ts" ? "number" : "i64";
    case "string":
      return lang === "ts" ? "string" : "String";
    case "array": {
      const item = wireType(schema.items, lang);
      if (lang === "rs") return `Vec<${item}>`;
      return schema.minItems === 1
        ? `[${item}, ...${item}[]]`
        : `Array<${item}>`;
    }
    case "object":
      if (schema.additionalProperties === true)
        return lang === "ts"
          ? "Record<string, unknown>"
          : "serde_json::Map<String, serde_json::Value>";
      if (
        typeof schema.additionalProperties === "object" &&
        schema.additionalProperties !== null
      ) {
        const value = wireType(schema.additionalProperties, lang);
        return lang === "ts"
          ? `Record<string, ${value}>`
          : `std::collections::BTreeMap<String, ${value}>`;
      }
  }
  throw new Error("Unsupported wire type: " + JSON.stringify(schema));
}

function fields(schema, properties, lang, visibility = "") {
  return Object.entries(properties)
    .map(([name, field]) => {
      const optional = !schema.required?.includes(name);
      const type = wireType(field, lang);
      if (lang === "ts") return `  ${name}${optional ? "?" : ""}: ${type};`;
      const attr = optional
        ? '  #[serde(skip_serializing_if = "Option::is_none")]\n'
        : "";
      return `${attr}  ${visibility}${snake(name)}: ${optional ? "Option<" + type + ">" : type},`;
    })
    .join("\n");
}

const derive = "#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]";
const enumDerive =
  "#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]";
function strings(name, values, lang) {
  if (lang === "ts")
    return `export type ${name} = ${values.map(quote).join(" | ")};\n`;
  return (
    `${enumDerive}\npub enum ${name} {\n` +
    values
      .map((v) => `  #[serde(rename = ${quote(v)})]\n  ${variantName(v)},`)
      .join("\n") +
    "\n}\n"
  );
}

function tagged(name, schema, lang) {
  const variants = variantsOf(schema);
  const tag = Object.keys(variants[0]).find(
    (key) => typeof variants[0][key].const === "string",
  );
  if (!tag) throw new Error("Expected string discriminator in " + name);
  const bodies = variants.map((variant) => {
    const properties = { ...schema.properties, ...variant };
    const literal = properties[tag].const;
    if (lang === "ts") return "{\n" + fields(schema, properties, lang) + "\n}";
    delete properties[tag];
    return (
      `  #[serde(rename = ${quote(literal)})]\n  ${variantName(literal)} {\n` +
      fields(schema, properties, lang) +
      "\n  },"
    );
  });
  return lang === "ts"
    ? `export type ${name} = ${bodies.join(" | ")};\n`
    : `${derive}\n#[serde(tag = ${quote(tag)}, rename_all_fields = "camelCase", deny_unknown_fields)]\npub enum ${name} {\n${bodies.join("\n")}\n}\n`;
}

function definition(name, schema, lang) {
  if (schema.enum) return strings(name, schema.enum, lang);
  if (schema.oneOf && name !== "RpcError") return tagged(name, schema, lang);
  if (schema.properties) {
    if (lang === "ts") {
      if (!Object.keys(schema.properties).length)
        return `export type ${name} = Record<string, never>;\n`;
      return `export interface ${name} {\n${fields(schema, schema.properties, lang)}\n}\n`;
    }
    return `${derive}\n#[serde(rename_all = "camelCase", deny_unknown_fields)]\npub struct ${name} {\n${fields(schema, schema.properties, lang, "pub ")}\n}\n`;
  }
  return `${lang === "ts" ? "export" : "pub"} type ${name} = ${wireType(schema, lang)};\n`;
}

export function publicTypes(defs, lang) {
  const errors = defs.RpcError.oneOf.map(({ properties: { code, data } }) => [
    code.const,
    data.properties.reason.const,
  ]);
  const types = Object.entries(defs).map(([name, schema]) => {
    if (name === "RpcError" && lang === "ts") {
      const branches = defs.RpcError.oneOf.map(
        ({ properties: { code, data } }) => {
          const range = (data.required ?? [])
            .map((key) => `${key}: ${wireType(data.properties[key], lang)};`)
            .join(" ");
          return `{ code: ${code.const}; message: string; data: { reason: ${quote(data.properties.reason.const)}; ${range} } }`;
        },
      );
      return "export type RpcError = " + branches.join(" | ") + ";\n";
    }
    return definition(name, schema, lang);
  });
  if (lang === "rs") {
    types.push(
      strings(
        "ErrorReason",
        errors.map(([, reason]) => reason),
        lang,
      ),
    );
    types.push(
      "impl ErrorReason {\n  pub fn code(self) -> i64 {\n    match self {\n" +
        errors
          .map(
            ([code, reason]) =>
              `      Self::${variantName(reason)} => ${code},`,
          )
          .join("\n") +
        "\n    }\n  }\n}\n",
    );
  }
  return banner + "\n" + types.join("\n");
}

export function methodTable({ methods, notifications }, defs) {
  const entries = Object.entries(methods);
  for (const name of [
    ...entries.flatMap(([, m]) => [m.params, m.result]),
    ...Object.values(notifications),
  ]) {
    if (!Object.hasOwn(defs, name)) throw new Error("No schema for " + name);
  }
  if (Object.keys(notifications).some((n) => Object.hasOwn(methods, n)))
    throw new Error("Ambiguous RPC name");
  // The macro uses the referenced Rust types as well as their schema names.
  return (
    banner +
    "rpc_methods! {\n" +
    entries
      .map(
        ([name, { params, result }]) =>
          `  ${variantName(name)} => (${quote(name)}, ${params}, ${result}),`,
      )
      .join("\n") +
    "\n}\n\npub fn notification_schema(name: &str) -> Option<&'static str> {\n  match name {\n" +
    Object.entries(notifications)
      .map(([name, type]) => `    ${quote(name)} => Some(stringify!(${type})),`)
      .join("\n") +
    "\n    _ => None,\n  }\n}\n"
  );
}
