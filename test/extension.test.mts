import * as assert from "assert";
// You can import and use all API from the 'vscode' module
// as well as import your extension to test it
import * as vscode from "vscode";

// import * as myExtension from '../../extension';

export interface CommandTestModule {
  suiteName: string;
  suite: () => void;
}

suite("Extension Test Suite", async () => {
  const commands_css_index = await import("./commands/css/index.test.mjs");
  const commands_genSettingsJson_index = await import(
    "./commands/getSettingsJson/index.test.mjs"
  );

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
});
