import * as assert from "node:assert/strict";
import * as vscode from "vscode";

import * as daemonRuntime from "./adapter/daemon-runtime.test.mjs";
import * as documentLifecycle from "./adapter/document-lifecycle.test.mjs";
import * as exportAck from "./adapter/export-ack.test.mjs";
import * as multiRootRuntime from "./adapter/multi-root-runtime.test.mjs";
import * as noteOptions from "./adapter/note-options.test.mjs";
import * as wiring from "./adapter/wiring.test.mjs";
import * as workspaceMigrationRuntime from "./adapter/workspace-migration-runtime.test.mjs";
import * as workspaceSelection from "./adapter/workspace-selection.test.mjs";
import * as contributions from "./contributions.test.mjs";
import * as release from "./release.test.mjs";

suite("Extension Test Suite", () => {
  suiteSetup(async () => {
    const extension = vscode.extensions.getExtension("Kashiwade.fleximark");
    assert.ok(extension);
    await extension.activate();
  });

  test("ships only adapter-owned settings", () => {
    const extension = vscode.extensions.getExtension("Kashiwade.fleximark");
    assert.ok(extension);
    assert.deepEqual(
      Object.keys(
        extension.packageJSON.contributes.configuration.properties,
      ).sort(),
      [
        "fleximark.autoOpenPreview",
        "fleximark.daemonPath",
        "fleximark.logLevel",
        "fleximark.previewColumn",
        "fleximark.previewTarget",
      ],
    );
  });

  test("declares daemon resolution and redacted adapter logging settings", () => {
    const extension = vscode.extensions.getExtension("Kashiwade.fleximark");
    assert.ok(extension);
    const properties =
      extension.packageJSON.contributes.configuration.properties;
    assert.equal(
      properties["fleximark.daemonPath"].scope,
      "machine-overridable",
    );
    assert.deepEqual(properties["fleximark.logLevel"].enum, [
      "off",
      "error",
      "info",
      "debug",
    ]);
  });

  test("does not ship the legacy JavaScript plugin runtime", async () => {
    const extension = vscode.extensions.getExtension("Kashiwade.fleximark");
    assert.ok(extension);
    await assert.rejects(
      Promise.resolve(
        vscode.workspace.fs.stat(
          vscode.Uri.joinPath(extension.extensionUri, "parserPlugin.js"),
        ),
      ),
    );
  });

  suite(contributions.suiteName, contributions.suite);
  suite(daemonRuntime.suiteName, daemonRuntime.suite);
  suite(documentLifecycle.suiteName, documentLifecycle.suite);
  suite(exportAck.suiteName, exportAck.suite);
  suite(noteOptions.suiteName, noteOptions.suite);
  suite(wiring.suiteName, wiring.suite);
  suite(workspaceSelection.suiteName, workspaceSelection.suite);
  suite(workspaceMigrationRuntime.suiteName, workspaceMigrationRuntime.suite);
  suite(multiRootRuntime.suiteName, multiRootRuntime.suite);
  suite(release.suiteName, release.suite);
});
