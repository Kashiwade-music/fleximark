import * as assert from "node:assert/strict";
import * as vscode from "vscode";

export const suiteName = "Editor contributions";

export function suite(): void {
  const extension = vscode.extensions.getExtension("Kashiwade.fleximark");

  test("declares and registers the stable FlexiMark command IDs", async () => {
    assert.ok(extension);
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
