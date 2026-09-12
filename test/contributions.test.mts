import * as assert from "node:assert/strict";
import * as fs from "node:fs/promises";
import * as path from "node:path";
import * as vscode from "vscode";

export const suiteName = "Static editor contributions";

async function readTypeScriptSources(directory: string): Promise<string> {
  const sources: string[] = [];
  for (const entry of await fs.readdir(directory, { withFileTypes: true })) {
    const entryPath = path.join(directory, entry.name);
    if (entry.isDirectory())
      sources.push(await readTypeScriptSources(entryPath));
    else if (entry.name.endsWith(".mts"))
      sources.push(await fs.readFile(entryPath, "utf8"));
  }
  return sources.join("\n");
}

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

  test("wires every stable provider and editor event exactly once", async () => {
    assert.ok(extension);
    const source = await readTypeScriptSources(
      path.join(extension.extensionPath, "adapters", "vscode", "src"),
    );
    const registrations = [
      "registrar.registerCodeActionsProvider",
      "registrar.registerCompletionItemProvider",
      "registrar.registerDocumentSymbolProvider",
      "registrar.registerHoverProvider",
      "registrar.window.onDidChangeActiveTextEditor",
      "registrar.window.onDidChangeTextEditorSelection",
      "registrar.window.onDidChangeTextEditorVisibleRanges",
      "registrar.workspace.onDidChangeTextDocument",
      "registrar.workspace.onDidChangeWorkspaceFolders",
      "registrar.workspace.onDidCloseTextDocument",
      "registrar.workspace.onDidGrantWorkspaceTrust",
      "registrar.workspace.onDidOpenTextDocument",
      "workspacePolicyWatcher.onDidChange",
      "workspacePolicyWatcher.onDidCreate",
      "workspacePolicyWatcher.onDidDelete",
    ];
    for (const registration of registrations) {
      assert.equal(
        source.split(registration).length - 1,
        1,
        `${registration} must be registered exactly once`,
      );
    }
  });

  test("composes each registrar once and subscribes every returned disposable", async () => {
    assert.ok(extension);
    const source = await fs.readFile(
      path.join(
        extension.extensionPath,
        "adapters",
        "vscode",
        "src",
        "extension.mts",
      ),
      "utf8",
    );
    for (const registrar of [
      "registerProviders",
      "registerCommands",
      "registerEditorEvents",
    ])
      assert.equal(
        source.split(`${registrar}(`).length - 1,
        1,
        `${registrar} must be composed exactly once`,
      );
    const compactSource = source.replace(/\s/g, "");
    assert.ok(
      compactSource.includes(
        "context.subscriptions.push(activatedAdapter);" +
          "context.subscriptions.push(" +
          "...registerProviders(activatedAdapter,undefined,isActive)," +
          "...registerCommands(activatedAdapter,undefined,isActive)," +
          ");",
      ),
    );
    assert.ok(
      compactSource.includes(
        "context.subscriptions.push(" +
          "...registerEditorEvents(activatedAdapter,undefined,isActive,{",
      ),
    );
  });

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
