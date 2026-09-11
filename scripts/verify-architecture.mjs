import assert from "node:assert/strict";
import console from "node:console";
import fs from "node:fs";
import { URL } from "node:url";

const packageJson = JSON.parse(
  fs.readFileSync(new URL("../package.json", import.meta.url)),
);
const inventory = JSON.parse(
  fs.readFileSync(
    new URL("../capabilities/feature-inventory.json", import.meta.url),
  ),
);
const dispositionCatalog = JSON.parse(
  fs.readFileSync(
    new URL("../capabilities/clean-break-catalog.json", import.meta.url),
  ),
);
const adapterSource = ["adapter.mts", "extension.mts"]
  .map((file) =>
    fs.readFileSync(
      new URL(`../adapters/vscode/src/${file}`, import.meta.url),
      "utf8",
    ),
  )
  .join("\n");
const rustProtocol = fs.readFileSync(
  new URL("../crates/fleximark-protocol/src/lib.rs", import.meta.url),
  "utf8",
);

assert.equal(packageJson.main, "./dist/extension.cjs");
assert.deepEqual(packageJson.activationEvents, [
  "onLanguage:markdown",
  "workspaceContains:.fleximark/config.toml",
]);
const forbiddenSettings = [
  "fleximark.browserPreviewPort",
  "fleximark.shouldSyncScroll",
  "fleximark.noteCategories",
  "fleximark.noteFileNamePrefix",
  "fleximark.noteFileNameSuffix",
  "fleximark.noteTemplates",
];
for (const key of forbiddenSettings) {
  assert.equal(
    packageJson.contributes.configuration.properties[key],
    undefined,
    `${key} is service-owned`,
  );
}
for (const dependency of Object.keys(packageJson.dependencies ?? {})) {
  assert.ok(
    !/^(remark|rehype|shiki|express$|ws$)/.test(dependency),
    `${dependency} belongs outside the adapter`,
  );
}
assert.equal(inventory.schemaVersion, 1);
const ids = inventory.capabilities.map(({ id }) => id);
assert.equal(new Set(ids).size, ids.length, "capability IDs must be unique");
for (const capability of inventory.capabilities) {
  assert.ok(capability.owner, `${capability.id} needs an owner`);
}
for (const [cleanBreakId, disposition] of Object.entries(
  dispositionCatalog.cleanBreaks,
)) {
  assert.ok(
    disposition.rationale &&
      disposition.replacement &&
      disposition.unsupportedBehavior,
    `${cleanBreakId} is incomplete`,
  );
  assert.ok(
    dispositionCatalog.releaseNotes[disposition.releaseNoteId],
    `${cleanBreakId} references an unknown release note`,
  );
}
for (const method of new Set(adapterSource.match(/fleximark\/[A-Za-z]+/g))) {
  assert.ok(
    rustProtocol.includes(`"${method}"`),
    `${method} is missing from fleximark-protocol`,
  );
}
for (const removedRuntime of ["src", "media"]) {
  const directory = new URL("../" + removedRuntime, import.meta.url);
  assert.ok(
    !fs.existsSync(directory) ||
      !fs
        .readdirSync(directory, { recursive: true, withFileTypes: true })
        .some((entry) => entry.isFile()),
    removedRuntime + ": legacy TypeScript runtime must not be shipped",
  );
}
assert.ok(
  !fs.existsSync(new URL("../parserPlugin.js", import.meta.url)),
  "parserPlugin.js: legacy TypeScript runtime must not be shipped",
);
for (const schema of [
  "config.schema.json",
  "protocol.schema.json",
  "plugin-manifest.schema.json",
  "feature-inventory.schema.json",
]) {
  const document = JSON.parse(
    fs.readFileSync(new URL(`../schemas/${schema}`, import.meta.url), "utf8"),
  );
  assert.equal(
    document.$schema,
    "https://json-schema.org/draft/2020-12/schema",
  );
}

const protocolSchema = JSON.parse(
  fs.readFileSync(new URL("../schemas/protocol.schema.json", import.meta.url)),
);
for (const dto of [
  "initializeParams",
  "initializeResult",
  "attachDocumentParams",
  "attachDocumentResult",
  "checkpointDocumentParams",
  "checkpointDocumentResult",
  "renderParams",
  "renderPublication",
  "renderStyle",
  "previewNavigationEvent",
  "sourceNavigationEvent",
  "renderNavigationEvent",
  "createPreviewParams",
  "createPreviewResult",
  "previewSessionParams",
  "selectionParams",
  "viewportParams",
  "previewEventParams",
  "executeCommandParams",
  "getNoteOptionsParams",
  "getNoteOptionsResult",
  "commandResult",
  "openDocumentParams",
  "changeDocumentParams",
  "requestFullTextParams",
  "closeDocumentParams",
  "methodResults",
]) {
  assert.ok(protocolSchema.$defs[dto], "protocol DTO " + dto + " is missing");
}
for (const match of rustProtocol.matchAll(
  /pub const [A-Z_]+: &str = "(fleximark\/[A-Za-z]+)";/g,
)) {
  assert.ok(
    JSON.stringify(protocolSchema).includes(match[1]),
    match[1] + " is missing from protocol.schema.json",
  );
}
assert.ok(!adapterSource.includes("params?: unknown"));

console.log(`Verified ${inventory.capabilities.length} current capabilities.`);
