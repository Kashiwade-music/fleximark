import * as assert from "node:assert/strict";
import * as vscode from "vscode";

import {
  type CommandRegistrar,
  registerCommands,
} from "../../adapters/vscode/src/commands.mjs";
import {
  type EditorEventRegistrar,
  registerEditorEvents,
} from "../../adapters/vscode/src/events.mjs";
import type { LspMethod } from "../../adapters/vscode/src/protocol.mjs";
import {
  type ProviderRegistrar,
  registerProviders,
} from "../../adapters/vscode/src/providers.mjs";

export const suiteName = "Extension wiring registrars";

type CommandCallback = (...args: unknown[]) => unknown;

function disposable(): vscode.Disposable {
  return { dispose: () => undefined };
}

function deferred<T>(): {
  promise: Promise<T>;
  resolve(value: T): void;
  reject(error: unknown): void;
} {
  let resolvePromise: (value: T) => void = () => undefined;
  let rejectPromise: (error: unknown) => void = () => undefined;
  const promise = new Promise<T>((resolve, reject) => {
    resolvePromise = resolve;
    rejectPromise = reject;
  });
  return { promise, resolve: resolvePromise, reject: rejectPromise };
}

async function settleCallbacks(): Promise<void> {
  await Promise.resolve();
  await Promise.resolve();
}

export function suite(): void {
  test("registers all nine commands, delegates callbacks, reports failures, and returns disposables", async () => {
    const callbacks = new Map<string, CommandCallback>();
    const registeredDisposables: vscode.Disposable[] = [];
    const registrar = {
      registerCommand(command: string, callback: CommandCallback) {
        callbacks.set(command, callback);
        const registration = disposable();
        registeredDisposables.push(registration);
        return registration;
      },
    } as unknown as CommandRegistrar;
    const calls: string[] = [];
    const previews: string[] = [];
    const reported: unknown[] = [];
    let reloads = 0;
    const rejection: { command?: string } = {};
    const failure = new Error("command failed");
    const adapter = {
      async execute(command: string) {
        calls.push(command);
        if (command === rejection.command) throw failure;
      },
      async forceReload() {
        reloads += 1;
      },
      async openPreview(target: string) {
        previews.push(target);
      },
      report(error: unknown) {
        reported.push(error);
      },
    } as Parameters<typeof registerCommands>[0];

    const registrations = registerCommands(adapter, registrar);
    const expectedCommandIds = [
      "fleximark.previewMarkdown",
      "fleximark.previewMarkdownOnVscode",
      "fleximark.previewMarkdownOnBrowser",
      "fleximark.forceReloadPreview",
      "fleximark.exportHtml",
      "fleximark.createNote",
      "fleximark.initializeWorkspace",
      "fleximark.collectAdmonitions",
      "fleximark.editTheme",
    ];
    assert.deepEqual([...callbacks.keys()], expectedCommandIds);
    assert.deepEqual(registrations, registeredDisposables);

    await callbacks.get("fleximark.previewMarkdown")?.();
    await callbacks.get("fleximark.previewMarkdownOnVscode")?.();
    await callbacks.get("fleximark.previewMarkdownOnBrowser")?.();
    await callbacks.get("fleximark.forceReloadPreview")?.();
    for (const command of [
      "exportHtml",
      "createNote",
      "initializeWorkspace",
      "collectAdmonitions",
      "editTheme",
    ])
      await callbacks.get(`fleximark.${command}`)?.();

    assert.deepEqual(previews, [
      vscode.workspace
        .getConfiguration("fleximark")
        .get("previewTarget", "embeddedHtml"),
      "embeddedHtml",
      "externalBrowser",
    ]);
    assert.equal(reloads, 1);
    assert.deepEqual(calls, [
      "exportHtml",
      "createNote",
      "initializeWorkspace",
      "collectAdmonitions",
      "editTheme",
    ]);

    rejection.command = "createNote";
    await assert.rejects(
      Promise.resolve(callbacks.get("fleximark.createNote")?.()),
      failure,
    );
    assert.deepEqual(reported, [failure]);
  });

  test("suppresses only reporting while preserving a retired command rejection", async () => {
    const callbacks = new Map<string, CommandCallback>();
    const registrar = {
      registerCommand(command: string, callback: CommandCallback) {
        callbacks.set(command, callback);
        return disposable();
      },
    } as unknown as CommandRegistrar;
    const pendingCommand = deferred<undefined>();
    const reported: unknown[] = [];
    let active = true;
    const failure = new Error("retired command failed");
    const adapter = {
      execute: () => pendingCommand.promise,
      forceReload: async () => undefined,
      openPreview: async () => undefined,
      report: (error: unknown) => reported.push(error),
    } as Parameters<typeof registerCommands>[0];
    registerCommands(adapter, registrar, () => active);

    const result = Promise.resolve(callbacks.get("fleximark.createNote")?.());
    active = false;
    pendingCommand.reject(failure);

    await assert.rejects(result, failure);
    assert.deepEqual(reported, []);
  });

  test("registers the four markdown providers with stable triggers and QuickFix metadata", async () => {
    const registeredDisposables: vscode.Disposable[] = [];
    const selectors: vscode.DocumentSelector[] = [];
    let completionProvider: vscode.CompletionItemProvider | undefined;
    let completionTriggers: readonly string[] | undefined;
    let hoverProvider: vscode.HoverProvider | undefined;
    let symbolProvider: vscode.DocumentSymbolProvider | undefined;
    let codeActionProvider: vscode.CodeActionProvider | undefined;
    let codeActionMetadata: vscode.CodeActionProviderMetadata | undefined;
    const addRegistration = (): vscode.Disposable => {
      const registration = disposable();
      registeredDisposables.push(registration);
      return registration;
    };
    const registrar = {
      registerCompletionItemProvider(
        selector: vscode.DocumentSelector,
        provider: vscode.CompletionItemProvider,
        ...triggers: string[]
      ) {
        selectors.push(selector);
        completionProvider = provider;
        completionTriggers = triggers;
        return addRegistration();
      },
      registerHoverProvider(
        selector: vscode.DocumentSelector,
        provider: vscode.HoverProvider,
      ) {
        selectors.push(selector);
        hoverProvider = provider;
        return addRegistration();
      },
      registerDocumentSymbolProvider(
        selector: vscode.DocumentSelector,
        provider: vscode.DocumentSymbolProvider,
      ) {
        selectors.push(selector);
        symbolProvider = provider;
        return addRegistration();
      },
      registerCodeActionsProvider(
        selector: vscode.DocumentSelector,
        provider: vscode.CodeActionProvider,
        metadata: vscode.CodeActionProviderMetadata,
      ) {
        selectors.push(selector);
        codeActionProvider = provider;
        codeActionMetadata = metadata;
        return addRegistration();
      },
    } as unknown as ProviderRegistrar;
    const requests: {
      method: LspMethod;
      document: vscode.TextDocument;
      params: object | undefined;
    }[] = [];
    const targetUri = vscode.Uri.parse("file:///provider.md");
    const editUri = vscode.Uri.parse("file:///fixed.md");
    const document = { uri: targetUri } as vscode.TextDocument;
    const position = new vscode.Position(2, 3);
    const range = new vscode.Range(1, 0, 1, 4);
    const results = new Map<LspMethod, unknown>([
      [
        "textDocument/completion",
        {
          items: [
            { label: "plain", insertText: "plain text" },
            { label: "snippet", insertText: "${1:value}", insertTextFormat: 2 },
          ],
        },
      ],
      ["textDocument/hover", { contents: { value: "**hover**" } }],
      [
        "textDocument/documentSymbol",
        [
          {
            name: "Heading",
            kind: vscode.SymbolKind.String,
            range: { start: range.start, end: range.end },
            selectionRange: { start: range.start, end: range.end },
          },
        ],
      ],
      [
        "textDocument/codeAction",
        [
          {
            title: "Escape HTML",
            kind: "quickfix",
            edit: {
              changes: {
                [editUri.toString()]: [{ range, newText: "&lt;tag&gt;" }],
              },
            },
          },
        ],
      ],
    ]);
    const adapter = {
      async requestLanguageFeature<T>(
        method: LspMethod,
        requestDocument: vscode.TextDocument,
        params?: object,
      ): Promise<T | undefined> {
        requests.push({ method, document: requestDocument, params });
        return results.get(method) as T | undefined;
      },
    };

    const registrations = registerProviders(adapter, registrar);
    assert.deepEqual(selectors, [
      { language: "markdown" },
      { language: "markdown" },
      { language: "markdown" },
      { language: "markdown" },
    ]);
    assert.deepEqual(completionTriggers, [":", "`"]);
    assert.deepEqual(codeActionMetadata, {
      providedCodeActionKinds: [vscode.CodeActionKind.QuickFix],
    });
    assert.deepEqual(registrations, registeredDisposables);

    assert.ok(completionProvider);
    assert.ok(hoverProvider);
    assert.ok(symbolProvider);
    assert.ok(codeActionProvider);
    const token = new vscode.CancellationTokenSource().token;
    const completionResult = await completionProvider.provideCompletionItems(
      document,
      position,
      token,
      {
        triggerCharacter: undefined,
        triggerKind: vscode.CompletionTriggerKind.Invoke,
      },
    );
    assert.ok(Array.isArray(completionResult));
    assert.deepEqual(
      completionResult.map((item) => ({
        insertText:
          item.insertText instanceof vscode.SnippetString
            ? item.insertText.value
            : item.insertText,
        label: item.label,
        snippet: item.insertText instanceof vscode.SnippetString,
      })),
      [
        { insertText: "plain text", label: "plain", snippet: false },
        { insertText: "${1:value}", label: "snippet", snippet: true },
      ],
    );
    const hoverResult = await hoverProvider.provideHover(
      document,
      position,
      token,
    );
    assert.ok(hoverResult instanceof vscode.Hover);
    assert.equal(hoverResult.contents.length, 1);
    const hoverContents = hoverResult.contents[0];
    assert.ok(hoverContents instanceof vscode.MarkdownString);
    assert.equal(hoverContents.value, "**hover**");
    const symbolResult = await symbolProvider.provideDocumentSymbols(
      document,
      token,
    );
    assert.ok(Array.isArray(symbolResult));
    assert.ok(
      symbolResult.every((symbol) => symbol instanceof vscode.DocumentSymbol),
    );
    const documentSymbols = symbolResult as vscode.DocumentSymbol[];
    assert.deepEqual(
      documentSymbols.map(({ name, range: symbolRange }) => ({
        name,
        range: symbolRange,
      })),
      [{ name: "Heading", range }],
    );
    const diagnostic = new vscode.Diagnostic(range, "unsafe HTML");
    diagnostic.code = "raw-html";
    diagnostic.source = "fleximark";
    (diagnostic as vscode.Diagnostic & { data?: unknown }).data = {
      escapedText: "&lt;tag&gt;",
    };
    const codeActionResult = await codeActionProvider.provideCodeActions(
      document,
      range,
      {
        diagnostics: [diagnostic],
        only: vscode.CodeActionKind.QuickFix,
        triggerKind: vscode.CodeActionTriggerKind.Invoke,
      },
      token,
    );
    assert.ok(Array.isArray(codeActionResult));
    assert.equal(codeActionResult.length, 1);
    const action = codeActionResult[0];
    assert.ok(action instanceof vscode.CodeAction);
    assert.equal(action.title, "Escape HTML");
    assert.equal(action.kind, vscode.CodeActionKind.QuickFix);
    assert.deepEqual(
      action.edit?.get(editUri).map((edit) => ({
        newText: edit.newText,
        range: edit.range,
      })),
      [{ newText: "&lt;tag&gt;", range }],
    );
    assert.deepEqual(
      requests.map(({ method, params }) => ({ method, params })),
      [
        { method: "textDocument/completion", params: { position } },
        { method: "textDocument/hover", params: { position } },
        { method: "textDocument/documentSymbol", params: undefined },
        {
          method: "textDocument/codeAction",
          params: {
            context: {
              diagnostics: [
                {
                  code: "raw-html",
                  data: { escapedText: "&lt;tag&gt;" },
                  message: "unsafe HTML",
                  range,
                  source: "fleximark",
                },
              ],
            },
            range,
          },
        },
      ],
    );
    assert.ok(requests.every((request) => request.document === document));
  });

  test("preserves pending provider results and failures after retirement", async () => {
    let completionProvider: vscode.CompletionItemProvider | undefined;
    const registrar = {
      registerCompletionItemProvider(
        _selector: vscode.DocumentSelector,
        provider: vscode.CompletionItemProvider,
      ) {
        completionProvider = provider;
        return disposable();
      },
      registerHoverProvider: () => disposable(),
      registerDocumentSymbolProvider: () => disposable(),
      registerCodeActionsProvider: () => disposable(),
    } as unknown as ProviderRegistrar;
    let pendingRequest = deferred<unknown>();
    let active = true;
    const adapter = {
      requestLanguageFeature<T>(): Promise<T | undefined> {
        return pendingRequest.promise as Promise<T | undefined>;
      },
    };
    registerProviders(adapter, registrar, () => active);
    assert.ok(completionProvider);
    const document = {
      uri: vscode.Uri.parse("file:///retired-provider.md"),
    } as vscode.TextDocument;
    const position = new vscode.Position(0, 0);
    const token = new vscode.CancellationTokenSource().token;
    const context = {
      triggerCharacter: undefined,
      triggerKind: vscode.CompletionTriggerKind.Invoke,
    };

    const retiredFailure = new Error("retired provider failed");
    const retiredResult = completionProvider.provideCompletionItems(
      document,
      position,
      token,
      context,
    );
    active = false;
    pendingRequest.reject(retiredFailure);
    await assert.rejects(Promise.resolve(retiredResult), retiredFailure);

    active = true;
    pendingRequest = deferred<unknown>();
    const successfulResult = completionProvider.provideCompletionItems(
      document,
      position,
      token,
      context,
    );
    active = false;
    pendingRequest.resolve({ items: [{ label: "preserved" }] });
    const successfulItems = (await successfulResult) as vscode.CompletionItem[];
    assert.equal(successfulItems.length, 1);
    assert.equal(successfulItems[0].label, "preserved");

    active = true;
    pendingRequest = deferred<unknown>();
    const currentResult = completionProvider.provideCompletionItems(
      document,
      position,
      token,
      context,
    );
    const currentFailure = new Error("current provider failed");
    pendingRequest.reject(currentFailure);
    await assert.rejects(Promise.resolve(currentResult), currentFailure);
  });

  test("registers every editor event and watcher and delegates callbacks with error reporting", async () => {
    const handlers = new Map<string, (event: unknown) => unknown>();
    const registeredDisposables: vscode.Disposable[] = [];
    const addHandler = (
      name: string,
      handler: (event: unknown) => unknown,
    ): vscode.Disposable => {
      handlers.set(name, handler);
      const registration = disposable();
      registeredDisposables.push(registration);
      return registration;
    };
    let watcherPattern: vscode.GlobPattern | undefined;
    const watcher = {
      dispose: () => undefined,
      onDidChange(listener: (uri: vscode.Uri) => unknown) {
        return addHandler(
          "watcher.change",
          listener as (event: unknown) => unknown,
        );
      },
      onDidCreate(listener: (uri: vscode.Uri) => unknown) {
        return addHandler(
          "watcher.create",
          listener as (event: unknown) => unknown,
        );
      },
      onDidDelete(listener: (uri: vscode.Uri) => unknown) {
        return addHandler(
          "watcher.delete",
          listener as (event: unknown) => unknown,
        );
      },
    } as vscode.FileSystemWatcher;
    const register =
      (name: string) =>
      (listener: (event: unknown) => unknown): vscode.Disposable =>
        addHandler(name, listener);
    const registrar = {
      workspace: {
        createFileSystemWatcher(pattern: vscode.GlobPattern) {
          watcherPattern = pattern;
          return watcher;
        },
        onDidOpenTextDocument: register("workspace.open"),
        onDidChangeTextDocument: register("workspace.change"),
        onDidCloseTextDocument: register("workspace.close"),
        onDidChangeWorkspaceFolders: register("workspace.folders"),
        onDidGrantWorkspaceTrust: register("workspace.trust"),
      },
      window: {
        onDidChangeActiveTextEditor: register("window.activeEditor"),
        onDidChangeTextEditorSelection: register("window.selection"),
        onDidChangeTextEditorVisibleRanges: register("window.visibleRanges"),
      },
    } as unknown as EditorEventRegistrar;
    const activations: (vscode.TextDocument | undefined)[] = [];
    const changes: vscode.TextDocument[] = [];
    const closes: vscode.TextDocument[] = [];
    const reconfigurations: vscode.WorkspaceFolder[] = [];
    const removals: vscode.WorkspaceFolder[] = [];
    const selections: vscode.TextEditorSelectionChangeEvent[] = [];
    const viewports: vscode.TextEditorVisibleRangesChangeEvent[] = [];
    const reported: unknown[] = [];
    let active = true;
    const pendingActivation: {
      value?: ReturnType<typeof deferred<undefined>>;
    } = {};
    const failures: {
      activation?: Error;
      reconfiguration?: Error;
    } = {};
    const adapter = {
      async activateDocument(document?: vscode.TextDocument) {
        activations.push(document);
        await pendingActivation.value?.promise;
        if (failures.activation) throw failures.activation;
      },
      changeDocument(document: vscode.TextDocument) {
        changes.push(document);
      },
      closeDocument(document: vscode.TextDocument) {
        closes.push(document);
      },
      async reconfigureWorkspace(workspace: vscode.WorkspaceFolder) {
        reconfigurations.push(workspace);
        if (failures.reconfiguration) throw failures.reconfiguration;
      },
      removeWorkspace(workspace: vscode.WorkspaceFolder) {
        removals.push(workspace);
      },
      report(error: unknown) {
        reported.push(error);
      },
      selectionChanged(event: vscode.TextEditorSelectionChangeEvent) {
        selections.push(event);
      },
      viewportChanged(event: vscode.TextEditorVisibleRangesChangeEvent) {
        viewports.push(event);
      },
    };

    const registrations = registerEditorEvents(
      adapter,
      registrar,
      () => active,
    );
    assert.equal(
      watcherPattern,
      "**/.fleximark/{config.toml,theme.css,plugins/**}",
    );
    assert.deepEqual(
      [...handlers.keys()],
      [
        "workspace.open",
        "window.activeEditor",
        "workspace.change",
        "workspace.close",
        "workspace.folders",
        "watcher.create",
        "watcher.change",
        "watcher.delete",
        "workspace.trust",
        "window.selection",
        "window.visibleRanges",
      ],
    );
    assert.equal(handlers.size + 4, 15);
    assert.deepEqual(registrations, [
      registeredDisposables[0],
      registeredDisposables[1],
      registeredDisposables[2],
      registeredDisposables[3],
      registeredDisposables[4],
      watcher,
      registeredDisposables[5],
      registeredDisposables[6],
      registeredDisposables[7],
      registeredDisposables[8],
      registeredDisposables[9],
      registeredDisposables[10],
    ]);

    const openedDocument = await vscode.workspace.openTextDocument({
      content: "wiring",
      language: "plaintext",
    });
    const openedEditor = await vscode.window.showTextDocument(openedDocument);
    const otherDocument = {
      uri: vscode.Uri.parse("untitled:other"),
    } as vscode.TextDocument;
    const folder = vscode.workspace.workspaceFolders?.[0];
    assert.ok(folder);
    const secondFolder = {
      index: folder.index + 1,
      name: "removed",
      uri: vscode.Uri.joinPath(folder.uri, "removed"),
    };
    const selectionEvent = {
      textEditor: openedEditor,
    } as vscode.TextEditorSelectionChangeEvent;
    const viewportEvent = {
      textEditor: openedEditor,
    } as vscode.TextEditorVisibleRangesChangeEvent;

    handlers.get("workspace.open")?.(otherDocument);
    handlers.get("workspace.open")?.(openedDocument);
    handlers.get("window.activeEditor")?.(undefined);
    handlers.get("window.activeEditor")?.(openedEditor);
    handlers.get("workspace.change")?.({ document: otherDocument });
    handlers.get("workspace.close")?.(otherDocument);
    handlers.get("workspace.folders")?.({ removed: [folder, secondFolder] });
    handlers.get("watcher.create")?.(
      vscode.Uri.joinPath(folder.uri, ".fleximark", "config.toml"),
    );
    handlers.get("watcher.change")?.(
      vscode.Uri.joinPath(folder.uri, ".fleximark", "theme.css"),
    );
    handlers.get("watcher.delete")?.(
      vscode.Uri.joinPath(folder.uri, ".fleximark", "plugins", "one.wasm"),
    );
    handlers.get("workspace.trust")?.(undefined);
    handlers.get("window.selection")?.(selectionEvent);
    handlers.get("window.visibleRanges")?.(viewportEvent);
    await settleCallbacks();

    assert.deepEqual(activations, [openedDocument, undefined, openedDocument]);
    assert.deepEqual(changes, [otherDocument]);
    assert.deepEqual(closes, [otherDocument]);
    assert.deepEqual(removals, [folder, secondFolder]);
    assert.deepEqual(reconfigurations, [folder, folder, folder, folder]);
    assert.deepEqual(selections, [selectionEvent]);
    assert.deepEqual(viewports, [viewportEvent]);

    failures.activation = new Error("activation failed");
    failures.reconfiguration = new Error("reconfiguration failed");
    handlers.get("window.activeEditor")?.(openedEditor);
    handlers.get("watcher.change")?.(
      vscode.Uri.joinPath(folder.uri, ".fleximark", "theme.css"),
    );
    await settleCallbacks();
    assert.equal(reported.length, 2);
    assert.ok(reported.includes(failures.activation));
    assert.ok(reported.includes(failures.reconfiguration));

    reported.length = 0;
    failures.activation = undefined;
    pendingActivation.value = deferred<undefined>();
    const retiredFailure = new Error("retired event failed");
    handlers.get("window.activeEditor")?.(openedEditor);
    active = false;
    pendingActivation.value.reject(retiredFailure);
    await settleCallbacks();
    assert.deepEqual(reported, []);
  });
}
