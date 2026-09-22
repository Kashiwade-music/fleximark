import * as assert from "node:assert/strict";
import * as vscode from "vscode";

export const suiteName = "Editor contributions";

export function suite(): void {
  const extension = vscode.extensions.getExtension("Kashiwade.fleximark");

  test("declares the stable package contributions and registers commands", async () => {
    assert.ok(extension);
    const properties =
      extension.packageJSON.contributes.configuration.properties;
    assert.deepEqual(Object.keys(properties).sort(), [
      "fleximark.autoOpenPreview",
      "fleximark.daemonPath",
      "fleximark.logLevel",
      "fleximark.previewColumn",
      "fleximark.previewTarget",
    ]);
    assert.equal(
      properties["fleximark.daemonPath"].scope,
      "machine-overridable",
    );
    assert.equal(properties["fleximark.previewTarget"].scope, "resource");
    assert.deepEqual(properties["fleximark.logLevel"].enum, [
      "off",
      "error",
      "info",
      "debug",
    ]);
    const expected = [
      "fleximark.collectAdmonitions",
      "fleximark.createNote",
      "fleximark.editTheme",
      "fleximark.exportHtml",
      "fleximark.forceReloadPreview",
      "fleximark.initializeWorkspace",
      "fleximark.previewMarkdown",
      "fleximark.previewMarkdownOnBrowser",
      "fleximark.previewMarkdownOnVscode",
    ];
    assert.deepEqual(
      extension.packageJSON.contributes.commands
        .map(({ command }: { command: string }) => command)
        .sort(),
      expected,
    );

    const registered = new Set(await vscode.commands.getCommands(true));
    assert.deepEqual(
      expected.filter((command) => registered.has(command)),
      expected,
    );
  });
}
