import base from "@mossop/config/react-native/eslint";

export default [
  {
    ignores: [
      ".prettierrc.js",
      "app.config.js",
      "babel.config.js",
      "eslint.config.mjs",
    ],
  },

  ...base,

  {
    languageOptions: {
      parserOptions: {
        tsconfigRootDir: import.meta.dirname,
        project: ["./tsconfig.json"],
      },
    },

    rules: {
      "react-hooks/preserve-manual-memoization": "off",
      "@typescript-eslint/no-empty-function": "off",
    },
  },
];
