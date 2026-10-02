import * as assert from "node:assert/strict";
import * as vscode from "vscode";

import { FlexiMarkAdapter } from "../../adapters/vscode/src/adapter.mjs";

export const suiteName = "Browser source navigation runtime";

export function suite(): void {
  test("scrolls the real editor after a browser navigation HTTP POST and daemon event", async function () {
    this.timeout(20_000);
    const extension = vscode.extensions.getExtension("Kashiwade.fleximark");
    const workspace = vscode.workspace.workspaceFolders?.[0];
    assert.ok(extension);
    assert.ok(workspace);
    const uri = vscode.Uri.joinPath(
      workspace.uri,
      "browser-navigation-runtime.md",
    );
    await vscode.workspace.fs.writeFile(
      uri,
      Buffer.from(
        Array.from(
          { length: 160 },
          (_, index) => `Paragraph ${index} 日本語 😀\n\n`,
        ).join(""),
      ),
    );
    const adapter = new FlexiMarkAdapter({
      extensionUri: extension.extensionUri,
      extension: { packageJSON: extension.packageJSON },
      extensionMode: vscode.ExtensionMode.Test,
    } as unknown as vscode.ExtensionContext);
    try {
      const document = await vscode.workspace.openTextDocument(uri);
      const editor = await vscode.window.showTextDocument(document);
      await adapter.start(workspace);
      const url = await adapter.openExternalPreviewForTest();
      assert.equal(
        adapter.recoveryStateForTest().previewRenderRevisions[uri.toString()],
        undefined,
      );
      const response = await fetch(`${url}/frame`);
      assert.equal(response.status, 200);
      const { frame } = (await response.json()) as {
        frame: {
          previewSessionId: string;
          renderRevision: number;
          navigation: {
            nodeId: string;
            sourceRange: { start: { line: number } };
          }[];
        };
      };
      const target = frame.navigation.find(
        (entry) => entry.sourceRange.start.line >= 160,
      );
      assert.ok(target, "the actual render frame contains a distant paragraph");
      assert.ok(
        editor.visibleRanges.every(
          (range) => range.start.line < target.sourceRange.start.line,
        ),
      );

      const posted = await fetch(`${url}/navigation`, {
        method: "POST",
        headers: {
          "content-type": "application/json",
          origin: new URL(url).origin,
        },
        body: JSON.stringify({
          type: "revealNode",
          previewSessionId: frame.previewSessionId,
          renderRevision: frame.renderRevision,
          nodeId: target.nodeId,
        }),
      });
      assert.equal(posted.status, 204);
      const targetLine = target.sourceRange.start.line;
      const deadline = Date.now() + 5_000;
      while (
        !editor.visibleRanges.some(
          (range) =>
            range.start.line >= targetLine - 10 &&
            range.start.line <= targetLine &&
            range.end.line >= targetLine,
        )
      ) {
        assert.ok(
          Date.now() < deadline,
          `browser navigation did not scroll the editor to line ${targetLine}; visible ranges: ${JSON.stringify(editor.visibleRanges)}`,
        );
        await new Promise((resolve) => setTimeout(resolve, 25));
      }
      assert.equal(
        adapter.recoveryStateForTest().previewRenderRevisions[uri.toString()],
        frame.renderRevision,
      );
    } finally {
      adapter.dispose();
      const tab = vscode.window.tabGroups.all
        .flatMap((group) => group.tabs)
        .find(
          (candidate) =>
            candidate.input instanceof vscode.TabInputText &&
            candidate.input.uri.toString() === uri.toString(),
        );
      if (tab) await vscode.window.tabGroups.close(tab, true);
      await vscode.workspace.fs.delete(uri, { useTrash: false });
    }
  });
}
