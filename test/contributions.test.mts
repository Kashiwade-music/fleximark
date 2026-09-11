import * as assert from "node:assert/strict";
import * as path from "node:path";
import * as vscode from "vscode";

export const suiteName = "Static editor contributions";

export function suite(): void {
  const extension = vscode.extensions.getExtension("Kashiwade.fleximark");

  test("all grammar contribution files exist", async () => {
    assert.ok(extension);
    for (const grammar of extension.packageJSON.contributes.grammars)
      await vscode.workspace.fs.stat(
        vscode.Uri.file(path.join(extension.extensionPath, grammar.path)),
      );
  });

  test("all snippet contribution files exist", async () => {
    assert.ok(extension);
    for (const snippet of extension.packageJSON.contributes.snippets)
      await vscode.workspace.fs.stat(
        vscode.Uri.file(path.join(extension.extensionPath, snippet.path)),
      );
  });

  test("the ABC language contribution has configuration and grammar", async () => {
    assert.ok(extension);
    const language = extension.packageJSON.contributes.languages.find(
      ({ id }: { id: string }) => id === "abc",
    );
    assert.ok(language);
    await vscode.workspace.fs.stat(
      vscode.Uri.file(
        path.join(extension.extensionPath, language.configuration),
      ),
    );
    assert.ok(
      extension.packageJSON.contributes.grammars.some(
        ({ language: id }: { language?: string }) => id === "abc",
      ),
    );
  });
}
