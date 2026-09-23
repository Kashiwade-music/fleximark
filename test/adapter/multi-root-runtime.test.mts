import * as assert from "node:assert/strict";
import * as path from "node:path";
import * as vscode from "vscode";

import type { FlexiMarkTestApi } from "../../adapters/vscode/src/extension.mjs";

export const suiteName = "Single-daemon multi-root runtime";

const sameUri = (left: vscode.Uri, right: vscode.Uri): boolean =>
  left.toString() === right.toString();

async function pollUntil<T>(
  read: () => T,
  complete: (value: T) => boolean,
  label: string,
  timeoutMilliseconds = 10_000,
): Promise<T> {
  return await new Promise<T>((resolve, reject) => {
    let settled = false;
    const timers: {
      interval?: ReturnType<typeof setInterval>;
      timeout?: ReturnType<typeof setTimeout>;
    } = {};
    const cleanup = (): void => {
      if (timers.interval !== undefined) clearInterval(timers.interval);
      if (timers.timeout !== undefined) clearTimeout(timers.timeout);
    };
    const resolveOnce = (value: T): void => {
      if (settled) return;
      settled = true;
      cleanup();
      resolve(value);
    };
    const rejectOnce = (error: unknown): void => {
      if (settled) return;
      settled = true;
      cleanup();
      reject(error);
    };
    const check = (): void => {
      if (settled) return;
      try {
        const value = read();
        if (complete(value)) resolveOnce(value);
      } catch (error) {
        rejectOnce(error);
      }
    };
    timers.interval = setInterval(check, 25);
    timers.timeout = setTimeout(() => {
      rejectOnce(new Error(`Timed out waiting for ${label}`));
    }, timeoutMilliseconds);
    check();
  });
}

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
  await pollUntil(
    () => api.recoveryState().previewRenderRevisions[document.toString()],
    (revision) => revision !== undefined,
    `preview render for ${document.toString()}`,
    5_000,
  );
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
      const afterRemoval = await pollUntil(
        () => ({
          labels: vscode.window.tabGroups.all
            .flatMap((group) => group.tabs)
            .map((tab) => tab.label)
            .filter((label) => label.startsWith("FlexiMark:")),
          recovery: api.recoveryState(),
        }),
        ({ labels, recovery }) =>
          !labels.some((label) => label.includes("alpha.md")) &&
          labels.some((label) => label.includes("beta.md")) &&
          recovery.documentSessions[alphaDocument.toString()] === undefined &&
          recovery.previewSessions[alphaDocument.toString()] === undefined &&
          recovery.previewRenderRevisions[alphaDocument.toString()] ===
            undefined &&
          recovery.documentSessions[betaDocument.toString()] !== undefined &&
          recovery.previewSessions[betaDocument.toString()] !== undefined,
        "scoped alpha removal while beta remains active",
      );
      const remaining = afterRemoval.labels;
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
