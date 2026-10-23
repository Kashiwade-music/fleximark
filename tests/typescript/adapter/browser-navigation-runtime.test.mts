import * as assert from "node:assert/strict";
import * as vscode from "vscode";

import { FlexiMarkAdapter } from "../../../adapters/vscode/src/adapter.mjs";
import { registerEditorEvents } from "../../../adapters/vscode/src/events.mjs";
import { untilBrowser, withBrowser } from "./browser-dom-harness.mjs";

export const suiteName = "Browser source navigation runtime";

export function suite(): void {
  if (process.env.FLEXIMARK_BROWSER_DOM === "true")
    test("renders unsaved math in Chromium, navigates from DOM, and recovers after daemon restart", async function () {
      this.timeout(90_000);
      const extension = vscode.extensions.getExtension("Kashiwade.fleximark");
      const workspace = vscode.workspace.workspaceFolders?.[0];
      assert.ok(extension);
      assert.ok(workspace);
      const uri = vscode.Uri.joinPath(
        workspace.uri,
        "browser-dom-acceptance.md",
      );
      await vscode.workspace.fs.writeFile(uri, Buffer.from("# Saved text\n"));
      const adapter = new FlexiMarkAdapter({
        extensionUri: extension.extensionUri,
        extension: { packageJSON: extension.packageJSON },
        extensionMode: vscode.ExtensionMode.Test,
      } as unknown as vscode.ExtensionContext);
      const subscriptions = registerEditorEvents(adapter);
      try {
        const document = await vscode.workspace.openTextDocument(uri);
        const editor = await vscode.window.showTextDocument(document);
        await editor.edit((edit) =>
          edit.replace(
            new vscode.Range(
              document.positionAt(0),
              document.positionAt(document.getText().length),
            ),
            "# Unsaved browser acceptance\n\n$$x^2 + y^2 = z^2$$\n\n" +
              Array.from(
                { length: 120 },
                (_, index) => `DOM paragraph ${index}\n\n`,
              ).join(""),
          ),
        );
        assert.ok(document.isDirty);
        await adapter.start(workspace);
        const url = await adapter.openExternalPreviewForTest();
        await withBrowser(async (evaluate, navigate) => {
          let heading = "Unsaved browser acceptance";
          let formula = "x^2";
          const rendered = async () =>
            Boolean(
              await evaluate(
                `document.body.innerText.includes(${JSON.stringify(heading)}) && document.querySelector('.katex annotation')?.textContent.includes(${JSON.stringify(formula)}) && Array.from(document.querySelectorAll('[data-fleximark-node-id]')).some(n => n.textContent.includes('DOM paragraph 100'))`,
              ),
            );
          await navigate(url);
          await untilBrowser(rendered);
          heading = "Live SSE browser acceptance";
          formula = "x^3";
          assert.equal(
            await editor.edit((edit) =>
              edit.replace(
                new vscode.Range(
                  document.positionAt(0),
                  document.positionAt(document.getText().length),
                ),
                document
                  .getText()
                  .replace("Unsaved browser acceptance", heading)
                  .replace("x^2", formula),
              ),
            ),
            true,
          );
          // Observe the existing page update through SSE before any navigation/reload.
          await untilBrowser(rendered);
          await evaluate(
            "Array.from(document.querySelectorAll('[data-fleximark-node-id]')).find(n => n.textContent.trim() === 'DOM paragraph 100').click()",
          );
          await untilBrowser(() => editor.selection.active.line >= 200);
          const instance = adapter.recoveryStateForTest().daemonInstanceId;
          adapter.crashDaemonForTest();
          await untilBrowser(() => {
            const state = adapter.recoveryStateForTest();
            return Boolean(
              state.daemonInstanceId &&
              state.daemonInstanceId !== instance &&
              state.previewSessions[uri.toString()],
            );
          });
          await navigate(await adapter.openExternalPreviewForTest());
          await untilBrowser(rendered);
          assert.ok(
            document.isDirty,
            "recovery must retain editor-owned unsaved source",
          );
          assert.equal(
            Buffer.from(await vscode.workspace.fs.readFile(uri)).toString(),
            "# Saved text\n",
          );
        });
      } finally {
        for (const subscription of subscriptions) subscription.dispose();
        adapter.dispose();
        const fixture = vscode.workspace.textDocuments.find(
          (document) => document.uri.toString() === uri.toString(),
        );
        if (fixture?.isDirty) {
          await vscode.window.showTextDocument(fixture);
          await vscode.commands.executeCommand(
            "workbench.action.revertAndCloseActiveEditor",
          );
        }
        const tabs = vscode.window.tabGroups.all
          .flatMap((group) => group.tabs)
          .filter(
            (tab) =>
              tab.input instanceof vscode.TabInputText &&
              tab.input.uri.toString() === uri.toString(),
          );
        for (const tab of tabs) await vscode.window.tabGroups.close(tab, true);
        await vscode.workspace.fs.delete(uri, { useTrash: false });
      }
    });
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
