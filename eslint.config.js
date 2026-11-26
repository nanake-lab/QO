import tsParser from "@typescript-eslint/parser";

export default [
  { ignores: ["**/dist/**", "coverage/**"] },
  {
    files: ["**/*.{js,mjs,ts}"],
    rules: {
      "no-debugger": "error",
      "no-dupe-keys": "error",
      "no-fallthrough": "error",
      "no-unreachable": "error",
      eqeqeq: "error",
    },
  },
  {
    files: ["**/*.ts"],
    languageOptions: { parser: tsParser },
  },
];
