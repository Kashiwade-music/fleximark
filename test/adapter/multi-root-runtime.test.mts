import * as assert from "node:assert/strict";
import * as os from "node:os";
import * as path from "node:path";
import * as vscode from "vscode";

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
      edit.insert(alphaDocument, new vscode.Position(1, 0), "unsaved\n");
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
