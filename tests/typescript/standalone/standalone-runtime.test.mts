import { registerEditorEvents } from "fleximark/src/events";
import type { FlexiMarkTestApi } from "fleximark/src/extension";
import * as assert from "node:assert/strict";
import { mkdtemp, readdir, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import process from "node:process";
import * as vscode from "vscode";

import { TestAdapter as FlexiMarkAdapter } from "../support/editor-adapter-harness.mjs";
import { until } from "../support/polling.mjs";

export const suiteName = "Standalone Markdown without a workspace";

function assertNoWorkspace(): void {
  assert.equal(vscode.workspace.workspaceFolders?.length ?? 0, 0);
  assert.equal(vscode.workspace.workspaceFile, undefined);
  if (process.env.FLEXIMARK_EXPECT_RESTRICTED === "1")
    assert.equal(vscode.workspace.isTrusted, false);
}

async function frameText(url: string): Promise<string> {
  const response = await fetch(`${url}/frame`);
  assert.equal(response.status, 200);
  const payload = (await response.json()) as { frame: unknown };
  return JSON.stringify(payload.frame);
}

export function suite(): void {
  test("previews standalone files, unsaved edits, switches and crash recovery without a folder", async function () {
    this.timeout(60_000);
    assertNoWorkspace();
    const directory = await mkdtemp(join(tmpdir(), "fleximark-standalone-"));
    const first = vscode.Uri.file(join(directory, "first.md"));
    const second = vscode.Uri.file(join(directory, "second.md"));
    const source = vscode.Uri.file(join(directory, "source.rs"));
    const uris = [first, second, source];
    const keys = uris.map((uri) => uri.toString());
    const errors: unknown[] = [];
    const closedDocuments = new Set<string>();
    let adapter: FlexiMarkAdapter | undefined;
    let subscriptions: vscode.Disposable[] = [];
    try {
      await writeFile(first.fsPath, "# Standalone initial first\n");
      await writeFile(second.fsPath, "# Standalone initial second\n");
      await writeFile(
        source.fsPath,
        'fn main() { println!("standalone source"); }\n',
      );
      const document = await vscode.workspace.openTextDocument(first);
      const editor = await vscode.window.showTextDocument(document);
      const extension = vscode.extensions.getExtension<FlexiMarkTestApi>(
        "Kashiwade.fleximark",
      );
      assert.ok(extension);
      const api = await extension.activate();
      adapter = new FlexiMarkAdapter({
        extensionUri: extension.extensionUri,
        extension: { packageJSON: extension.packageJSON },
        extensionMode: vscode.ExtensionMode.Test,
      } as unknown as vscode.ExtensionContext);
      const externalAdapter = adapter;
      externalAdapter.report = (error: unknown) => {
        errors.push(error);
      };
      subscriptions = registerEditorEvents(externalAdapter);
      subscriptions.push(
        vscode.workspace.onDidCloseTextDocument((closed) => {
          closedDocuments.add(closed.uri.toString());
        }),
      );
      await externalAdapter.activateEditor(editor);
      await until(
        () => Boolean(api.recoveryState().documentSessions[keys[0]]),
        "packaged extension standalone document activation",
      );
      const url = await externalAdapter.openExternalPreviewForTest();
      assert.match(await frameText(url), /Standalone initial first/);
      assertNoWorkspace();

      const edit = new vscode.WorkspaceEdit();
      edit.insert(
        first,
        document.positionAt(document.getText().length),
        "\nUnsaved standalone edit\n",
      );
      assert.equal(await vscode.workspace.applyEdit(edit), true);
      assert.equal(document.isDirty, true);
      await until(
        async () => (await frameText(url)).includes("Unsaved standalone edit"),
        "external preview unsaved edit frame",
      );

      await vscode.window.showTextDocument(document);
      await vscode.commands.executeCommand("fleximark.previewMarkdownOnVscode");
      await until(
        () => (api.recoveryState().previewRenderRevisions[keys[0]] ?? 0) > 0,
        "embedded preview without a folder selection",
      );
      const embeddedRevision =
        api.recoveryState().previewRenderRevisions[keys[0]] ?? 0;
      const secondEdit = new vscode.WorkspaceEdit();
      secondEdit.insert(
        first,
        document.positionAt(document.getText().length),
        "\nEmbedded unsaved edit\n",
      );
      assert.equal(await vscode.workspace.applyEdit(secondEdit), true);
      await until(
        () =>
          (api.recoveryState().previewRenderRevisions[keys[0]] ?? 0) >
          embeddedRevision,
        "embedded preview unsaved edit revision",
      );

      const otherDocument = await vscode.workspace.openTextDocument(second);
      const otherEditor = await vscode.window.showTextDocument(otherDocument);
      await externalAdapter.activateEditor(otherEditor);
      const secondUrl = await externalAdapter.openExternalPreviewForTest();
      assert.match(await frameText(secondUrl), /Standalone initial second/);
      await until(
        () => Boolean(api.recoveryState().documentSessions[keys[1]]),
        "second standalone Markdown session",
      );
      const sourceDocument = await vscode.workspace.openTextDocument(source);
      assert.equal(sourceDocument.languageId, "rust");
      const sourceEditor = await vscode.window.showTextDocument(sourceDocument);
      await externalAdapter.activateEditor(sourceEditor);
      assert.equal(api.recoveryState().documentSessions[keys[2]], undefined);
      assert.equal(
        externalAdapter.recoveryStateForTest().documentSessions[keys[2]],
        undefined,
      );
      assertNoWorkspace();

      const beforeExternal = externalAdapter.recoveryStateForTest();
      const beforeEmbedded = api.recoveryState();
      externalAdapter.crashDaemonForTest();
      api.crashDaemon();
      await until(() => {
        const state = externalAdapter.recoveryStateForTest();
        return Boolean(
          state.daemonInstanceId &&
          state.daemonInstanceId !== beforeExternal.daemonInstanceId &&
          state.documentSessions[keys[0]] &&
          state.documentSessions[keys[1]] &&
          state.previewSessions[keys[0]],
        );
      }, "external standalone document and preview replay");
      await until(() => {
        const state = api.recoveryState();
        return Boolean(
          state.daemonInstanceId &&
          state.daemonInstanceId !== beforeEmbedded.daemonInstanceId &&
          state.documentSessions[keys[0]] &&
          (state.previewRenderRevisions[keys[0]] ?? 0) > 0,
        );
      }, "embedded standalone document and preview replay");
      await vscode.window.showTextDocument(document);
      const replayUrl = await externalAdapter.openExternalPreviewForTest();
      assert.match(await frameText(replayUrl), /Embedded unsaved edit/);
      assert.equal(document.isDirty, true);
      assert.deepEqual((await readdir(directory)).sort(), [
        "first.md",
        "second.md",
        "source.rs",
      ]);
      assertNoWorkspace();

      assert.equal(await document.save(), true);
      // Closing editor tabs may retain text models. A language change emits the
      // actual VS Code document-close/open boundary; plaintext must stay detached
      // from Markdown processing when the same URI is reopened.
      await vscode.languages.setTextDocumentLanguage(document, "plaintext");
      await vscode.languages.setTextDocumentLanguage(
        otherDocument,
        "plaintext",
      );
      assert.ok(closedDocuments.has(keys[0]));
      assert.ok(closedDocuments.has(keys[1]));
      await vscode.window.tabGroups.close(
        vscode.window.tabGroups.all.flatMap((group) => group.tabs),
        true,
      );
      try {
        await until(
          () =>
            keys.every(
              (key) =>
                !api.recoveryState().documentSessions[key] &&
                !externalAdapter.recoveryStateForTest().documentSessions[key] &&
                !api.recoveryState().previewSessions[key] &&
                !externalAdapter.recoveryStateForTest().previewSessions[key],
            ),
          "standalone document-close and preview cleanup",
        );
      } catch (error) {
        throw new Error(
          JSON.stringify({
            message: String(error),
            packaged: api.recoveryState(),
            external: externalAdapter.recoveryStateForTest(),
            documents: vscode.workspace.textDocuments.map((entry) => ({
              uri: entry.uri.toString(),
              closed: entry.isClosed,
            })),
            tabs: vscode.window.tabGroups.all
              .flatMap((group) => group.tabs)
              .map((tab) => tab.label),
          }),
          { cause: error },
        );
      }
      assert.deepEqual(errors, []);
      assertNoWorkspace();
    } finally {
      for (const subscription of subscriptions) subscription.dispose();
      adapter?.dispose();
      for (const document of vscode.workspace.textDocuments) {
        if (keys.includes(document.uri.toString()) && document.isDirty)
          await document.save();
      }
      await vscode.window.tabGroups.close(
        vscode.window.tabGroups.all.flatMap((group) => group.tabs),
        true,
      );
      await rm(directory, {
        recursive: true,
        force: true,
        maxRetries: 10,
        retryDelay: 100,
      });
    }
  });
}
