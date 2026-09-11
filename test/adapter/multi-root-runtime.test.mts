import * as assert from "node:assert/strict";
import * as path from "node:path";
import * as vscode from "vscode";

import type { FlexiMarkTestApi } from "../../adapters/vscode/src/extension.mjs";

export const suiteName = "Single-daemon multi-root runtime";

const sameUri = (left: vscode.Uri, right: vscode.Uri): boolean =>
  left.toString() === right.toString();

async function removeWorkspaceFolder(uri: vscode.Uri): Promise<void> {
  const currentIndex = vscode.workspace.workspaceFolders?.findIndex((folder) =>
    sameUri(folder.uri, uri),
  );
  if (currentIndex === undefined || currentIndex < 0) return;

  await new Promise<void>((resolve, reject) => {
    let removalEventObserved = false;
    const finishIfRemoved = (): void => {
      const remains = vscode.workspace.workspaceFolders?.some((folder) =>
        sameUri(folder.uri, uri),
      );
      if (!removalEventObserved || remains) return;
      clearInterval(stateCheck);
      clearTimeout(timeout);
      subscription.dispose();
      resolve();
    };
    const subscription = vscode.workspace.onDidChangeWorkspaceFolders(
      (event) => {
        if (event.removed.some((folder) => sameUri(folder.uri, uri)))
          removalEventObserved = true;
        finishIfRemoved();
      },
    );
    const stateCheck = setInterval(finishIfRemoved, 25);
    const timeout = setTimeout(() => {
      clearInterval(stateCheck);
      subscription.dispose();
      reject(
        new Error(`Timed out removing workspace folder ${uri.toString()}`),
      );
    }, 5_000);
    if (!vscode.workspace.updateWorkspaceFolders(currentIndex, 1)) {
      clearInterval(stateCheck);
      clearTimeout(timeout);
      subscription.dispose();
      reject(new Error(`Could not remove workspace folder ${uri.toString()}`));
    }
  });
}

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

async function addWorkspaceFolders(
  folders: readonly { uri: vscode.Uri; name: string }[],
): Promise<void> {
  const expected = new Set(folders.map(({ uri }) => uri.toString()));
  await new Promise<void>((resolve, reject) => {
    const observed = new Set<string>();
    const finishIfAdded = (): void => {
      const current = new Set(
        (vscode.workspace.workspaceFolders ?? []).map(({ uri }) =>
          uri.toString(),
        ),
      );
      if ([...expected].some((uri) => !observed.has(uri) || !current.has(uri)))
        return;
      clearInterval(stateCheck);
      clearTimeout(timeout);
      subscription.dispose();
      resolve();
    };
    const subscription = vscode.workspace.onDidChangeWorkspaceFolders(
      (event) => {
        for (const folder of event.added) {
          const uri = folder.uri.toString();
          if (expected.has(uri)) observed.add(uri);
        }
        finishIfAdded();
      },
    );
    const stateCheck = setInterval(finishIfAdded, 25);
    const timeout = setTimeout(() => {
      clearInterval(stateCheck);
      subscription.dispose();
      reject(new Error("Timed out adding FlexiMark workspace folders"));
    }, 5_000);
    const index = vscode.workspace.workspaceFolders?.length ?? 0;
    if (!vscode.workspace.updateWorkspaceFolders(index, 0, ...folders)) {
      clearInterval(stateCheck);
      clearTimeout(timeout);
      subscription.dispose();
      reject(new Error("Could not add FlexiMark workspace folders"));
    }
  });
}

async function waitForPreviewRendered(
  api: FlexiMarkTestApi,
  document: vscode.Uri,
): Promise<void> {
  for (let attempt = 0; attempt < 50; attempt += 1) {
    if (
      api.recoveryState().previewRenderRevisions[document.toString()] !==
      undefined
    )
      return;
    await new Promise((resolve) => setTimeout(resolve, 100));
  }
  throw new Error(`Preview did not render for ${document.toString()}`);
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

      let beforeCrash = api.recoveryState();
      for (let attempt = 0; attempt < 50; attempt += 1) {
        beforeCrash = api.recoveryState();
        if (
          beforeCrash.previewRenderRevisions[alphaDocument.toString()] &&
          beforeCrash.previewRenderRevisions[betaDocument.toString()]
        )
          break;
        await new Promise((resolve) => setTimeout(resolve, 100));
      }
      assert.ok(beforeCrash.daemonInstanceId);
      assert.ok(beforeCrash.documentSessions[alphaDocument.toString()]);
      assert.ok(beforeCrash.documentSessions[betaDocument.toString()]);
      assert.ok(beforeCrash.previewSessions[alphaDocument.toString()]);
      assert.ok(beforeCrash.previewSessions[betaDocument.toString()]);
      assert.ok(beforeCrash.previewRenderRevisions[alphaDocument.toString()]);
      assert.ok(beforeCrash.previewRenderRevisions[betaDocument.toString()]);

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
      assert.notEqual(
        afterCrash.previewSessions[betaDocument.toString()],
        beforeCrash.previewSessions[betaDocument.toString()],
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
      await new Promise((resolve) => setTimeout(resolve, 1_000));
      const remaining = vscode.window.tabGroups.all
        .flatMap((group) => group.tabs)
        .map((tab) => tab.label)
        .filter((label) => label.startsWith("FlexiMark:"));
      assert.ok(!remaining.some((label) => label.includes("alpha.md")));
      assert.ok(remaining.some((label) => label.includes("beta.md")));
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
