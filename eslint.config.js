import tsParser from "@typescript-eslint/parser";

export default [
  {
    ignores: [
      "**/dist/**",
      "coverage/**",
      "target/**",
      "apps/desktop/gen/**",
      "packages/protocol/generated/**",
    ],
  },
  {
    files: ["**/*.{js,mjs,ts,tsx}"],
    rules: {
      "no-debugger": "error",
      "no-dupe-keys": "error",
      "no-fallthrough": "error",
      "no-unreachable": "error",
      eqeqeq: "error",
    },
  },
  {
    files: ["**/*.{ts,tsx}"],
    languageOptions: { parser: tsParser },
  },
];
