import * as assert from "assert";
// You can import and use all API from the 'vscode' module
// as well as import your extension to test it
import * as vscode from "vscode";

import * as commands_css_index from "./command/lib/css/index.test.mjs";
import * as commands_genSettingsJson_index from "./command/lib/settings/index.test.mjs";
import * as commands_utils_getBlockLineAndOffset from "./command/utils/getBlockLineAndOffset.test.mjs";
import * as completion_lib_checkCurrentLineLangMode from "./completion/lib/checkCurrentLineLangMode.test.mjs";

export interface CommandTestModule {
  suiteName: string;
  suite: () => void;
}

suite("Extension Test Suite", () => {
  suiteSetup(async () => {
    // activate the extension before running tests
    await vscode.extensions.getExtension("Kashiwade.fleximark")?.activate();
  });

  test("Sample test", () => {
    assert.strictEqual(-1, [1, 2, 3].indexOf(5));
    assert.strictEqual(-1, [1, 2, 3].indexOf(0));
  });

  suite(commands_css_index.suiteName, commands_css_index.suite);
  suite(
    commands_genSettingsJson_index.suiteName,
    commands_genSettingsJson_index.suite,
  );
  suite(
    commands_utils_getBlockLineAndOffset.suiteName,
    commands_utils_getBlockLineAndOffset.suite,
  );

  // =======================

  suite(
    completion_lib_checkCurrentLineLangMode.suiteName,
    completion_lib_checkCurrentLineLangMode.suite,
  );
});
