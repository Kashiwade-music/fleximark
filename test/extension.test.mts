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

suite("Extension Test Suite", () => {
  suiteSetup(async () => {
    const extension = vscode.extensions.getExtension("Kashiwade.fleximark");
    assert.ok(extension);
    await extension.activate();
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
});
