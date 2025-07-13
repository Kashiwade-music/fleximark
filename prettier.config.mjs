import prettierPluginSortImports from "@trivago/prettier-plugin-sort-imports";

export default {
  plugins: [prettierPluginSortImports],
  importOrder: [
    "^@core/(.*)$",
    "<THIRD_PARTY_MODULES>",
    "^@server/(.*)$",
    "^@ui/(.*)$",
    "^[./]",
  ],
  importOrderSeparation: true,
  importOrderSortSpecifiers: true,
};
