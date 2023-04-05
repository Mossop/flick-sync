module.exports = {
  parserOptions: {
    tsconfigRootDir: __dirname,
    project: ["./tsconfig.json"],
  },

  ignorePatterns: ["node_modules"],

  extends: [require.resolve("@mossop/config/web-ts/eslintrc")],

  rules: {
    "no-console": "off",
  },
};
