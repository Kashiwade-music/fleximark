import assert from "node:assert/strict";
import console from "node:console";
import fs from "node:fs";
import { URL } from "node:url";

const packageJson = JSON.parse(
  fs.readFileSync(new URL("../package.json", import.meta.url)),
);
const matrix = JSON.parse(
  fs.readFileSync(
    new URL("../capabilities/release-baseline.yaml", import.meta.url),
  ),
);
const testEvidence = JSON.parse(
  fs.readFileSync(
    new URL("../capabilities/test-evidence.json", import.meta.url),
  ),
);
const frozenInventory = JSON.parse(
  fs.readFileSync(
    new URL("../capabilities/v0.16.14-inventory.json", import.meta.url),
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
assert.equal(matrix.schemaVersion, 1);
const ids = matrix.capabilities.map(({ id }) => id);
assert.equal(new Set(ids).size, ids.length, "capability IDs must be unique");
for (const capability of matrix.capabilities) {
  assert.ok(["keep", "change", "remove"].includes(capability.decision));
  assert.ok(capability.owner, `${capability.id} needs an owner`);
  assert.ok(
    capability.baselineEvidence.length > 0,
    `${capability.id} needs baseline evidence`,
  );
  assert.ok(
    capability.contract.length > 0,
    `${capability.id} needs a contract`,
  );
  assert.ok(capability.tests.length > 0, `${capability.id} needs verification`);
  for (const testId of capability.tests) {
    const test = testEvidence[testId];
    assert.ok(test, capability.id + " references unknown test " + testId);
    const sourceUrl = new URL("../" + test.file, import.meta.url);
    assert.ok(fs.existsSync(sourceUrl), testId + " source does not exist");
    const source = fs.readFileSync(sourceUrl, "utf8");
    const marker = test.marker.replace(/[.*+?^$()|[\]\\]/g, "\\$&");
    const declaration = test.file.endsWith(".rs")
      ? new RegExp("#\\[test\\]\\s*fn\\s+" + marker + "\\s*\\(")
      : new RegExp("test\\(\\s*[\"']" + marker + "[\"']");
    assert.ok(
      declaration.test(source),
      testId + " does not name a test in " + test.file,
    );
  }
  if (capability.decision !== "keep") {
    assert.ok(
      capability.cleanBreakId,
      `${capability.id} needs a clean-break disposition ID`,
    );
    assert.ok(
      capability.releaseNoteId,
      `${capability.id} needs a release note ID`,
    );
    const disposition = dispositionCatalog.cleanBreaks[capability.cleanBreakId];
    assert.ok(disposition, `${capability.cleanBreakId} is not catalogued`);
    assert.equal(disposition.releaseNoteId, capability.releaseNoteId);
    assert.ok(
      disposition.rationale &&
        disposition.replacement &&
        disposition.unsupportedBehavior,
    );
    assert.ok(
      dispositionCatalog.releaseNotes[capability.releaseNoteId],
      `${capability.releaseNoteId} is not catalogued`,
    );
  }
}
const evidence = matrix.capabilities.flatMap(
  (capability) => capability.baselineEvidence,
);
for (const legacyId of [
  ...frozenInventory.commands,
  ...frozenInventory.settings,
  ...frozenInventory.grammars,
  ...frozenInventory.snippets,
]) {
  assert.equal(
    evidence.filter((item) => item.endsWith(legacyId)).length,
    1,
    `${legacyId} must have exactly one frozen-baseline disposition`,
  );
}
for (const language of frozenInventory.languages) {
  assert.equal(
    evidence.filter((item) => item.endsWith(`/languages/${language}`)).length,
    1,
    `${language} language must have exactly one frozen-baseline disposition`,
  );
}
for (const [feature, capabilityId] of Object.entries(
  frozenInventory.documentedFeatures,
)) {
  assert.ok(
    ids.includes(capabilityId),
    `${feature} is missing capability ${capabilityId}`,
  );
}
for (const contribution of packageJson.contributes.commands) {
  assert.ok(
    evidence.some((item) => item.includes(contribution.command)),
    `${contribution.command} is missing from the capability matrix`,
  );
}
for (const setting of Object.keys(
  packageJson.contributes.configuration.properties,
)) {
  assert.ok(
    evidence.some((item) => item.includes(setting)),
    `${setting} is missing from the capability matrix`,
  );
}
for (const contribution of packageJson.contributes.grammars) {
  assert.ok(
    evidence.some((item) => item.includes(contribution.path)),
    contribution.path + " grammar is missing from the capability matrix",
  );
}
for (const contribution of packageJson.contributes.snippets) {
  assert.ok(
    evidence.some((item) => item.includes(contribution.path)),
    contribution.path + " snippet is missing from the capability matrix",
  );
}
for (const contribution of packageJson.contributes.languages) {
  assert.ok(
    evidence.some((item) => item.includes(contribution.id)),
    contribution.id + " language is missing from the capability matrix",
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
  "capability-matrix.schema.json",
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

console.log(
  `Verified ${matrix.capabilities.length} architecture capabilities.`,
);
