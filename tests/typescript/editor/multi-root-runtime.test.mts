import type { FlexiMarkTestApi } from "fleximark/src/extension";
import * as assert from "node:assert/strict";
import * as path from "node:path";
import * as vscode from "vscode";

import { pollUntil } from "../support/polling.mjs";
import {
  addWorkspaceFolders,
  removeWorkspaceFolder,
} from "../support/workspace-folders.mjs";

export const suiteName = "Single-daemon multi-root runtime";

async function collectWorkspaceRemovalErrors(
  uris: readonly vscode.Uri[],
): Promise<unknown[]> {
  const errors: unknown[] = [];
  for (const uri of uris) {
    try {
      await removeWorkspaceFolder(uri);
    } catch (error) {
      errors.push(error);
    }
  }
  return errors;
}

async function waitForPreviewRendered(
  api: FlexiMarkTestApi,
  document: vscode.Uri,
): Promise<void> {
  try {
    await pollUntil(
      () => api.recoveryState().previewRenderRevisions[document.toString()],
      (revision) => revision !== undefined,
      `preview render for ${document.toString()}`,
      5_000,
    );
  } catch (cause) {
    const state = api.recoveryState();
    throw new Error(
      JSON.stringify({
        message: "Preview render did not become observable",
        expectedDocumentUri: document.toString(),
        documentSessionKeys: Object.keys(state.documentSessions),
        previewSessionKeys: Object.keys(state.previewSessions),
        previewRenderRevisionKeys: Object.keys(state.previewRenderRevisions),
        state,
      }),
      { cause },
    );
  }
}

async function saveAndCloseTestEditors(
  documents: readonly vscode.Uri[],
): Promise<unknown[]> {
  const errors: unknown[] = [];
  const unsavedDocumentKeys = new Set<string>();
  const documentKeys = new Set(
    documents.map((document) => document.toString()),
  );
  for (const document of vscode.workspace.textDocuments) {
    if (!documentKeys.has(document.uri.toString()) || !document.isDirty)
      continue;
    try {
      if (!(await document.save()))
        throw new Error(
          `Could not save test document ${document.uri.toString()}`,
        );
    } catch (error) {
      unsavedDocumentKeys.add(document.uri.toString());
      errors.push(error);
    }
  }

  const documentNames = new Set(
    documents.map((document) => path.basename(document.fsPath)),
  );
  const tabs = vscode.window.tabGroups.all
    .flatMap((group) => group.tabs)
    .filter((tab) => {
      if (tab.input instanceof vscode.TabInputText)
        return (
          documentKeys.has(tab.input.uri.toString()) &&
          !unsavedDocumentKeys.has(tab.input.uri.toString())
        );
      return (
        tab.input instanceof vscode.TabInputWebview &&
        tab.label.startsWith("FlexiMark:") &&
        [...documentNames].some((name) => tab.label.includes(name))
      );
    });
  for (const tab of tabs) {
    try {
      if (!(await vscode.window.tabGroups.close(tab, true)))
        throw new Error(`Could not close test tab ${tab.label}`);
    } catch (error) {
      errors.push(error);
    }
  }
  return errors;
}

export function suite(): void {
  test("keeps unsaved documents and previews across root replay and scoped removal", async function () {
    this.timeout(60_000);
    const staleFolders = (vscode.workspace.workspaceFolders ?? []).filter(
      (folder) => folder.name.startsWith("FlexiMark "),
    );
    const staleCleanupErrors = await collectWorkspaceRemovalErrors(
      staleFolders.map(({ uri }) => uri),
    );
    if (staleCleanupErrors.length)
      throw new AggregateError(
        staleCleanupErrors,
        "Could not remove stale FlexiMark workspace folders",
      );
    const workspaceFile = vscode.workspace.workspaceFile;
    assert.ok(workspaceFile, "integration tests must open a workspace file");
    assert.equal(workspaceFile.scheme, "file");
    const root = vscode.Uri.file(
      path.join(
        path.dirname(workspaceFile.fsPath),
        `fleximark-multiroot-${Date.now()}`,
      ),
    );
    const alpha = vscode.Uri.joinPath(root, "alpha");
    const beta = vscode.Uri.joinPath(root, "beta");
    const alphaDocument = vscode.Uri.joinPath(alpha, "alpha.md");
    const betaDocument = vscode.Uri.joinPath(beta, "beta.md");
    let rootCreated = false;
    let testError: unknown;
    let testFailed = false;
    const cleanupErrors: unknown[] = [];

    try {
      await vscode.workspace.fs.createDirectory(alpha);
      rootCreated = true;
      await vscode.workspace.fs.createDirectory(beta);
      await vscode.workspace.fs.writeFile(
        alphaDocument,
        Buffer.from("# Alpha\n"),
      );
      await vscode.workspace.fs.writeFile(
        betaDocument,
        Buffer.from("# Beta\n"),
      );
      await addWorkspaceFolders([
        { uri: alpha, name: "FlexiMark Alpha" },
        { uri: beta, name: "FlexiMark Beta" },
      ]);

      const extension = vscode.extensions.getExtension<FlexiMarkTestApi>(
        "Kashiwade.fleximark",
      );
      assert.ok(extension);
      const api = extension.isActive
        ? extension.exports
        : await extension.activate();
      const alphaText = await vscode.workspace.openTextDocument(alphaDocument);
      await vscode.window.showTextDocument(alphaText);
      await vscode.commands.executeCommand("fleximark.previewMarkdownOnVscode");
      await waitForPreviewRendered(api, alphaDocument);
      const edit = new vscode.WorkspaceEdit();
      edit.insert(alphaDocument, new vscode.Position(1, 0), "# Unsaved\n");
      assert.equal(await vscode.workspace.applyEdit(edit), true);

      const betaText = await vscode.workspace.openTextDocument(betaDocument);
      await vscode.window.showTextDocument(betaText);
      await vscode.commands.executeCommand("fleximark.previewMarkdownOnVscode");
      await waitForPreviewRendered(api, betaDocument);
      await vscode.window.showTextDocument(alphaText);
      await vscode.commands.executeCommand("fleximark.forceReloadPreview");

      const previewLabels = await pollUntil(
        () =>
          vscode.window.tabGroups.all
            .flatMap((group) => group.tabs)
            .map((tab) => tab.label)
            .filter((label) => label.startsWith("FlexiMark:")),
        (labels) =>
          labels.some((label) => label.includes("alpha.md")) &&
          labels.some((label) => label.includes("beta.md")),
        "both preview tabs",
      );
      assert.ok(previewLabels.some((label) => label.includes("alpha.md")));
      assert.ok(previewLabels.some((label) => label.includes("beta.md")));

      const beforeCrash = await pollUntil(
        () => api.recoveryState(),
        (state) =>
          state.previewRenderRevisions[alphaDocument.toString()] !==
            undefined &&
          state.previewRenderRevisions[betaDocument.toString()] !== undefined,
        "both preview render revisions before crash",
        5_000,
      );
      assert.ok(beforeCrash.daemonInstanceId);
      assert.ok(beforeCrash.documentSessions[alphaDocument.toString()]);
      assert.ok(beforeCrash.documentSessions[betaDocument.toString()]);
      assert.ok(beforeCrash.previewSessions[alphaDocument.toString()]);
      assert.ok(beforeCrash.previewSessions[betaDocument.toString()]);
      assert.ok(beforeCrash.previewRenderRevisions[alphaDocument.toString()]);
      assert.ok(beforeCrash.previewRenderRevisions[betaDocument.toString()]);

      api.crashDaemon();
      let afterCrash = await pollUntil(
        () => api.recoveryState(),
        (state) =>
          state.connectionGeneration > beforeCrash.connectionGeneration &&
          Boolean(state.daemonInstanceId) &&
          Boolean(state.documentSessions[alphaDocument.toString()]) &&
          Boolean(state.documentSessions[betaDocument.toString()]) &&
          state.previewSessions[alphaDocument.toString()] !==
            beforeCrash.previewSessions[alphaDocument.toString()] &&
          state.previewSessions[betaDocument.toString()] !==
            beforeCrash.previewSessions[betaDocument.toString()],
        "daemon and preview replacement after crash",
      );
      assert.ok(
        afterCrash.connectionGeneration > beforeCrash.connectionGeneration,
        "daemon connection was not replaced after the crash",
      );
      assert.notEqual(
        afterCrash.daemonInstanceId,
        beforeCrash.daemonInstanceId,
      );
      assert.notDeepEqual(
        [
          afterCrash.daemonInstanceId,
          afterCrash.documentSessions[alphaDocument.toString()],
        ],
        [
          beforeCrash.daemonInstanceId,
          beforeCrash.documentSessions[alphaDocument.toString()],
        ],
      );
      assert.notDeepEqual(
        [
          afterCrash.daemonInstanceId,
          afterCrash.previewSessions[alphaDocument.toString()],
        ],
        [
          beforeCrash.daemonInstanceId,
          beforeCrash.previewSessions[alphaDocument.toString()],
        ],
      );
      assert.notDeepEqual(
        [
          afterCrash.daemonInstanceId,
          afterCrash.previewSessions[betaDocument.toString()],
        ],
        [
          beforeCrash.daemonInstanceId,
          beforeCrash.previewSessions[betaDocument.toString()],
        ],
      );
      await waitForPreviewRendered(api, alphaDocument);
      await waitForPreviewRendered(api, betaDocument);
      afterCrash = api.recoveryState();
      assert.notEqual(
        afterCrash.previewRenderRevisions[alphaDocument.toString()],
        undefined,
        "alpha preview replay remained blank after the crash",
      );
      assert.notEqual(
        afterCrash.previewRenderRevisions[betaDocument.toString()],
        undefined,
        "beta preview replay remained blank after the crash",
      );
      const symbols = await vscode.commands.executeCommand<
        vscode.DocumentSymbol[]
      >("vscode.executeDocumentSymbolProvider", alphaDocument);
      assert.ok(symbols?.some((symbol) => symbol.name === "Unsaved"));

      await removeWorkspaceFolder(alpha);
      const afterRemoval = await pollUntil(
        () => ({
          labels: vscode.window.tabGroups.all
            .flatMap((group) => group.tabs)
            .map((tab) => tab.label)
            .filter((label) => label.startsWith("FlexiMark:")),
          recovery: api.recoveryState(),
        }),
        ({ labels, recovery }) =>
          labels.some((label) => label.includes("alpha.md")) &&
          labels.some((label) => label.includes("beta.md")) &&
          recovery.connectionGeneration === afterCrash.connectionGeneration &&
          Boolean(recovery.daemonInstanceId) &&
          recovery.documentSessions[alphaDocument.toString()] !== undefined &&
          recovery.previewSessions[alphaDocument.toString()] !== undefined &&
          recovery.previewRenderRevisions[alphaDocument.toString()] !==
            undefined &&
          recovery.documentSessions[betaDocument.toString()] !== undefined &&
          recovery.previewSessions[betaDocument.toString()] !== undefined &&
          recovery.previewRenderRevisions[betaDocument.toString()] !==
            undefined,
        "alpha detaches without losing its open document or preview while beta remains active",
      );
      const remaining = afterRemoval.labels;
      assert.equal(
        vscode.workspace.getWorkspaceFolder(alphaDocument),
        undefined,
      );
      assert.equal(
        afterRemoval.recovery.daemonInstanceId,
        afterCrash.daemonInstanceId,
      );
      assert.ok(remaining.some((label) => label.includes("alpha.md")));
      assert.ok(remaining.some((label) => label.includes("beta.md")));
      const detachedSymbols = await vscode.commands.executeCommand<
        vscode.DocumentSymbol[]
      >("vscode.executeDocumentSymbolProvider", alphaDocument);
      assert.ok(detachedSymbols?.some((symbol) => symbol.name === "Unsaved"));
    } catch (error) {
      testFailed = true;
      testError = error;
    } finally {
      try {
        cleanupErrors.push(
          ...(await saveAndCloseTestEditors([alphaDocument, betaDocument])),
        );
      } catch (error) {
        cleanupErrors.push(error);
      }
      try {
        cleanupErrors.push(
          ...(await collectWorkspaceRemovalErrors([alpha, beta])),
        );
      } finally {
        if (rootCreated) {
          try {
            await vscode.workspace.fs.delete(root, {
              recursive: true,
              useTrash: false,
            });
          } catch (error) {
            cleanupErrors.push(error);
          }
        }
      }
    }
    if (testFailed && cleanupErrors.length)
      throw new AggregateError(
        [testError, ...cleanupErrors],
        "Multi-root test and cleanup failed",
      );
    if (testFailed) throw testError;
    if (cleanupErrors.length)
      throw new AggregateError(cleanupErrors, "Multi-root cleanup failed");
  });
}
