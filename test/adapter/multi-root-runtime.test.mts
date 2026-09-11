import * as assert from "node:assert/strict";
import * as os from "node:os";
import * as path from "node:path";
import * as vscode from "vscode";

import type { FlexiMarkTestApi } from "../../adapters/vscode/src/extension.mjs";

export const suiteName = "Single-daemon multi-root runtime";

export function suite(): void {
  test("keeps unsaved documents and previews across root replay and scoped removal", async function () {
    this.timeout(60_000);
    for (const index of [
      ...(vscode.workspace.workspaceFolders ?? []).keys(),
    ].reverse()) {
      const folder = vscode.workspace.workspaceFolders?.[index];
      if (folder?.name.startsWith("FlexiMark "))
        vscode.workspace.updateWorkspaceFolders(index, 1);
    }
    await new Promise((resolve) => setTimeout(resolve, 250));
    const root = vscode.Uri.file(
      path.join(os.tmpdir(), `fleximark-multiroot-${Date.now()}`),
    );
    const alpha = vscode.Uri.joinPath(root, "alpha");
    const beta = vscode.Uri.joinPath(root, "beta");
    const alphaDocument = vscode.Uri.joinPath(alpha, "alpha.md");
    const betaDocument = vscode.Uri.joinPath(beta, "beta.md");
    await vscode.workspace.fs.createDirectory(alpha);
    await vscode.workspace.fs.createDirectory(beta);
    await vscode.workspace.fs.writeFile(
      alphaDocument,
      Buffer.from("# Alpha\n"),
    );
    await vscode.workspace.fs.writeFile(betaDocument, Buffer.from("# Beta\n"));
    const originalCount = vscode.workspace.workspaceFolders?.length ?? 0;
    assert.equal(
      vscode.workspace.updateWorkspaceFolders(
        originalCount,
        0,
        { uri: alpha, name: "FlexiMark Alpha" },
        { uri: beta, name: "FlexiMark Beta" },
      ),
      true,
    );

    try {
      const alphaText = await vscode.workspace.openTextDocument(alphaDocument);
      await vscode.window.showTextDocument(alphaText);
      await vscode.commands.executeCommand("fleximark.previewMarkdownOnVscode");
      const edit = new vscode.WorkspaceEdit();
      edit.insert(alphaDocument, new vscode.Position(1, 0), "# Unsaved\n");
      assert.equal(await vscode.workspace.applyEdit(edit), true);

      const betaText = await vscode.workspace.openTextDocument(betaDocument);
      await vscode.window.showTextDocument(betaText);
      await vscode.commands.executeCommand("fleximark.previewMarkdownOnVscode");
      await vscode.window.showTextDocument(alphaText);
      await vscode.commands.executeCommand("fleximark.forceReloadPreview");

      let previewLabels: string[] = [];
      for (let attempt = 0; attempt < 20; attempt += 1) {
        previewLabels = vscode.window.tabGroups.all
          .flatMap((group) => group.tabs)
          .map((tab) => tab.label)
          .filter((label) => label.startsWith("FlexiMark:"));
        if (
          previewLabels.some((label) => label.includes("alpha.md")) &&
          previewLabels.some((label) => label.includes("beta.md"))
        )
          break;
        await new Promise((resolve) => setTimeout(resolve, 100));
      }
      assert.ok(previewLabels.some((label) => label.includes("alpha.md")));
      assert.ok(previewLabels.some((label) => label.includes("beta.md")));

      const extension = vscode.extensions.getExtension<FlexiMarkTestApi>(
        "Kashiwade.fleximark",
      );
      assert.ok(extension);
      const api = extension.isActive
        ? extension.exports
        : await extension.activate();
      const beforeCrash = api.recoveryState();
      assert.ok(beforeCrash.daemonInstanceId);
      assert.ok(beforeCrash.documentSessions[alphaDocument.toString()]);
      assert.ok(beforeCrash.documentSessions[betaDocument.toString()]);
      assert.ok(beforeCrash.previewSessions[alphaDocument.toString()]);
      assert.ok(beforeCrash.previewSessions[betaDocument.toString()]);

      api.crashDaemon();
      let afterCrash = api.recoveryState();
      for (let attempt = 0; attempt < 100; attempt += 1) {
        afterCrash = api.recoveryState();
        if (
          afterCrash.connectionGeneration > beforeCrash.connectionGeneration &&
          afterCrash.daemonInstanceId &&
          afterCrash.documentSessions[alphaDocument.toString()] &&
          afterCrash.documentSessions[betaDocument.toString()] &&
          afterCrash.previewSessions[alphaDocument.toString()] !==
            beforeCrash.previewSessions[alphaDocument.toString()] &&
          afterCrash.previewSessions[betaDocument.toString()] !==
            beforeCrash.previewSessions[betaDocument.toString()]
        )
          break;
        await new Promise((resolve) => setTimeout(resolve, 100));
      }
      assert.ok(
        afterCrash.connectionGeneration > beforeCrash.connectionGeneration,
        "daemon connection was not replaced after the crash",
      );
      assert.notEqual(
        afterCrash.daemonInstanceId,
        beforeCrash.daemonInstanceId,
      );
      assert.notEqual(
        afterCrash.documentSessions[alphaDocument.toString()],
        beforeCrash.documentSessions[alphaDocument.toString()],
      );
      assert.notEqual(
        afterCrash.previewSessions[alphaDocument.toString()],
        beforeCrash.previewSessions[alphaDocument.toString()],
      );
      const symbols = await vscode.commands.executeCommand<
        vscode.DocumentSymbol[]
      >("vscode.executeDocumentSymbolProvider", alphaDocument);
      assert.ok(symbols?.some((symbol) => symbol.name === "Unsaved"));

      const alphaIndex = vscode.workspace.workspaceFolders?.findIndex(
        (folder) => folder.uri.toString() === alpha.toString(),
      );
      assert.ok(alphaIndex !== undefined && alphaIndex >= 0);
      assert.equal(
        vscode.workspace.updateWorkspaceFolders(alphaIndex, 1),
        true,
      );
      await new Promise((resolve) => setTimeout(resolve, 1_000));
      const remaining = vscode.window.tabGroups.all
        .flatMap((group) => group.tabs)
        .map((tab) => tab.label)
        .filter((label) => label.startsWith("FlexiMark:"));
      assert.ok(!remaining.some((label) => label.includes("alpha.md")));
      assert.ok(remaining.some((label) => label.includes("beta.md")));
    } finally {
      for (const uri of [alpha, beta]) {
        const index = vscode.workspace.workspaceFolders?.findIndex(
          (folder) => folder.uri.toString() === uri.toString(),
        );
        if (index !== undefined && index >= 0)
          vscode.workspace.updateWorkspaceFolders(index, 1);
      }
      await vscode.workspace.fs.delete(root, {
        recursive: true,
        useTrash: false,
      });
    }
  });
}
