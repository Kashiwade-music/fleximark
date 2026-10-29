import eslint from "@eslint/js";
import eslintPluginPrettierRecommended from "eslint-plugin-prettier/recommended";
import tseslint from "typescript-eslint";

const packageImports = {
  group: [
    "**/contracts/typescript/**",
    "**/adapters/vscode/**",
    "**/web/preview/**",
    "**/web/workspace-ui/**",
  ],
  message: "Use workspace package imports across package boundaries.",
};

export default tseslint.config(
  {
    ignores: [
      ".build/**",
      ".artifacts/**",
      "tooling/xtask-core/**",
      "contracts/typescript/*.generated.mts",
      "contracts/distribution/*.generated.mjs",
    ],
  },
  eslint.configs.recommended,
  tseslint.configs.strict,
  tseslint.configs.stylistic,
  eslintPluginPrettierRecommended,
  {
    files: ["**/*.mts"],
    rules: {
      "no-restricted-imports": ["error", { patterns: [packageImports] }],
      "max-lines": [
        "error",
        { max: 999, skipBlankLines: true, skipComments: true },
      ],
    },
  },
  {
    files: ["contracts/typescript/**/*.mts"],
    rules: {
      "no-restricted-imports": [
        "error",
        {
          patterns: [
            packageImports,
            {
              group: [
                "**/adapters/**",
                "**/web/**",
                "fleximark",
                "fleximark/*",
                "@fleximark/preview",
                "@fleximark/preview/*",
                "@fleximark/workspace-ui",
                "@fleximark/workspace-ui/*",
              ],
              message:
                "Contracts must not import application or browser packages.",
            },
          ],
        },
      ],
    },
  },
  {
    files: ["web/preview/**/*.mts"],
    rules: {
      "no-restricted-imports": [
        "error",
        {
          paths: ["vscode"],
          patterns: [
            packageImports,
            {
              group: [
                "**/adapters/**",
                "fleximark",
                "fleximark/*",
                "**/workspace-ui/**",
                "@fleximark/workspace-ui",
                "@fleximark/workspace-ui/*",
              ],
              message:
                "Preview code must not import adapters or the workspace UI.",
            },
          ],
        },
      ],
    },
  },
  {
    files: ["web/workspace-ui/**/*.mts"],
    rules: {
      "no-restricted-imports": [
        "error",
        {
          patterns: [
            packageImports,
            {
              group: [
                "**/preview/**",
                "@fleximark/preview",
                "@fleximark/preview/*",
              ],
              message: "Browser packages must not import each other.",
            },
          ],
        },
      ],
    },
  },
);
