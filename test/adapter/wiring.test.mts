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
import { deferred, flushMicrotasks } from "./async-helpers.mjs";

export const suiteName = "Extension wiring registrars";

type CommandCallback = (...args: unknown[]) => unknown;

function disposable(): vscode.Disposable {
  return { dispose: () => undefined };
}

function commandRegistrar(
  callbacks: Map<string, CommandCallback>,
  registrations: vscode.Disposable[] = [],
): CommandRegistrar {
  return {
    registerCommand(command, callback) {
      callbacks.set(command, callback);
      const registration = disposable();
      registrations.push(registration);
      return registration;
    },
  };
}

export function suite(): void {
  test("registers commands, delegates representative callbacks, and reports failures", async () => {
    const callbacks = new Map<string, CommandCallback>();
    const registeredDisposables: vscode.Disposable[] = [];
    const registrar = commandRegistrar(callbacks, registeredDisposables);
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
    assert.equal(callbacks.size, 9);
    assert.equal(registrations.length, registeredDisposables.length);

    await callbacks.get("fleximark.previewMarkdown")?.();
    await callbacks.get("fleximark.previewMarkdownOnBrowser")?.();
    await callbacks.get("fleximark.forceReloadPreview")?.();
    await callbacks.get("fleximark.createNote")?.();

    assert.deepEqual(previews, [
      vscode.workspace
        .getConfiguration("fleximark")
        .get("previewTarget", "embeddedHtml"),
      "externalBrowser",
    ]);
    assert.equal(reloads, 1);
    assert.deepEqual(calls, ["createNote"]);

    rejection.command = "createNote";
    await assert.rejects(
      Promise.resolve(callbacks.get("fleximark.createNote")?.()),
      failure,
    );
    assert.deepEqual(reported, [failure]);
  });

  test("suppresses only reporting while preserving a retired command rejection", async () => {
    const callbacks = new Map<string, CommandCallback>();
    const registrar = commandRegistrar(callbacks);
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

  test("registers the markdown providers with stable triggers and QuickFix metadata", async () => {
    const registeredDisposables: vscode.Disposable[] = [];
    const selectors: vscode.DocumentSelector[] = [];
    let completionProvider: vscode.CompletionItemProvider | undefined;
    let completionTriggers: readonly string[] | undefined;
    let semanticProvider: vscode.DocumentSemanticTokensProvider | undefined;
    let semanticLegend: vscode.SemanticTokensLegend | undefined;
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
      registerDocumentSemanticTokensProvider(
        selector: vscode.DocumentSelector,
        provider: vscode.DocumentSemanticTokensProvider,
        legend: vscode.SemanticTokensLegend,
      ) {
        selectors.push(selector);
        semanticProvider = provider;
        semanticLegend = legend;
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
            {
              label: "plain",
              kind: vscode.CompletionItemKind.Snippet,
              detail: "Plain completion",
              filterText: "plain-filter",
              insertTextFormat: 1,
              textEdit: { range, newText: "plain text" },
            },
            {
              label: "snippet",
              kind: vscode.CompletionItemKind.Snippet,
              detail: "Snippet completion",
              filterText: "snippet-filter",
              insertTextFormat: 2,
              textEdit: { range, newText: "${1:value}" },
            },
          ],
        },
      ],
      ["textDocument/semanticTokens/full", { data: [0, 0, 3, 2, 0] }],
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
    assert.deepEqual(
      selectors,
      Array.from({ length: 5 }, () => ({ language: "markdown" })),
    );
    assert.deepEqual(completionTriggers, [":", "`", ">"]);
    assert.deepEqual(codeActionMetadata, {
      providedCodeActionKinds: [vscode.CodeActionKind.QuickFix],
    });
    assert.deepEqual(registrations, registeredDisposables);

    assert.ok(completionProvider);
    assert.ok(semanticProvider);
    const legend = semanticLegend;
    assert.ok(legend);
    assert.deepEqual(legend.tokenTypes, [
      "keyword",
      "string",
      "operator",
      "type",
      "property",
    ]);
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
        detail: item.detail,
        filterText: item.filterText,
        insertText:
          item.insertText instanceof vscode.SnippetString
            ? item.insertText.value
            : item.insertText,
        label: item.label,
        range: item.range,
        snippet: item.insertText instanceof vscode.SnippetString,
      })),
      [
        {
          detail: "Plain completion",
          filterText: "plain-filter",
          insertText: "plain text",
          label: "plain",
          range,
          snippet: false,
        },
        {
          detail: "Snippet completion",
          filterText: "snippet-filter",
          insertText: "${1:value}",
          label: "snippet",
          range,
          snippet: true,
        },
      ],
    );
    const semanticResult = await semanticProvider.provideDocumentSemanticTokens(
      document,
      token,
    );
    assert.ok(semanticResult instanceof vscode.SemanticTokens);
    assert.deepEqual([...semanticResult.data], [0, 0, 3, 2, 0]);
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
        { method: "textDocument/semanticTokens/full", params: undefined },
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
      registerDocumentSemanticTokensProvider: () => disposable(),
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
    const provider = completionProvider;
    const provide = (): vscode.ProviderResult<
      vscode.CompletionItem[] | vscode.CompletionList
    > => provider.provideCompletionItems(document, position, token, context);

    const retiredFailure = new Error("retired provider failed");
    const retiredResult = provide();
    active = false;
    pendingRequest.reject(retiredFailure);
    await assert.rejects(Promise.resolve(retiredResult), retiredFailure);

    active = true;
    pendingRequest = deferred<unknown>();
    const successfulResult = provide();
    active = false;
    pendingRequest.resolve({
      items: [
        {
          label: "preserved",
          kind: vscode.CompletionItemKind.Snippet,
          detail: "Preserved completion",
          filterText: "preserved",
          insertTextFormat: 1,
          textEdit: {
            range: new vscode.Range(position, position),
            newText: "preserved",
          },
        },
      ],
    });
    const successfulItems = (await successfulResult) as vscode.CompletionItem[];
    assert.equal(successfulItems.length, 1);
    assert.equal(successfulItems[0].label, "preserved");

    active = true;
    pendingRequest = deferred<unknown>();
    const currentResult = provide();
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
    const calls = new Map<string, unknown[]>();
    const record = (name: string, value?: unknown): void => {
      const values = calls.get(name) ?? [];
      values.push(value);
      calls.set(name, values);
    };
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
        record("activate", document);
      },
      async activateEditor(editor?: vscode.TextEditor) {
        record("activateEditor", editor);
        await pendingActivation.value?.promise;
        if (failures.activation) throw failures.activation;
      },
      changeDocument: (document: vscode.TextDocument) =>
        record("change", document),
      closeDocument: (document: vscode.TextDocument) =>
        record("close", document),
      async reconfigureWorkspace(workspace: vscode.WorkspaceFolder) {
        record("reconfigure", workspace);
        if (failures.reconfiguration) throw failures.reconfiguration;
      },
      removeWorkspace: (workspace: vscode.WorkspaceFolder) =>
        record("remove", workspace),
      report(error: unknown) {
        reported.push(error);
      },
      selectionChanged: (event: vscode.TextEditorSelectionChangeEvent) =>
        record("selection", event),
      viewportChanged: (event: vscode.TextEditorVisibleRangesChangeEvent) =>
        record("viewport", event),
    };

    const registrations = registerEditorEvents(
      adapter,
      registrar,
      () => active,
      {
        added: (workspace) => record("migration.add", workspace),
        removed: (workspace) => record("migration.remove", workspace),
        trustGranted: () => record("migration.trust"),
      },
    );
    assert.equal(
      watcherPattern,
      "**/.fleximark/{config.toml,theme.css,plugins/**}",
    );
    assert.equal(handlers.size, 11);
    assert.equal(registrations.length, 12);
    assert.ok(registrations.includes(watcher));
    assert.ok(
      registeredDisposables.every((item) => registrations.includes(item)),
    );

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
    handlers.get("workspace.folders")?.({
      added: [secondFolder],
      removed: [folder],
    });
    handlers.get("watcher.change")?.(
      vscode.Uri.joinPath(folder.uri, ".fleximark", "theme.css"),
    );
    handlers.get("workspace.trust")?.(undefined);
    handlers.get("window.selection")?.(selectionEvent);
    handlers.get("window.visibleRanges")?.(viewportEvent);
    await flushMicrotasks(2);

    assert.deepEqual(calls.get("activate"), [openedDocument]);
    assert.deepEqual(calls.get("activateEditor"), [undefined, openedEditor]);
    assert.deepEqual(calls.get("change"), [otherDocument]);
    assert.deepEqual(calls.get("close"), [otherDocument]);
    assert.deepEqual(calls.get("remove"), [folder]);
    assert.deepEqual(calls.get("migration.add"), [secondFolder]);
    assert.deepEqual(calls.get("migration.remove"), [folder]);
    assert.deepEqual(calls.get("migration.trust"), [undefined]);
    assert.deepEqual(calls.get("reconfigure"), [folder, folder]);
    assert.deepEqual(calls.get("selection"), [selectionEvent]);
    assert.deepEqual(calls.get("viewport"), [viewportEvent]);

    failures.activation = new Error("activation failed");
    failures.reconfiguration = new Error("reconfiguration failed");
    handlers.get("window.activeEditor")?.(openedEditor);
    handlers.get("watcher.change")?.(
      vscode.Uri.joinPath(folder.uri, ".fleximark", "theme.css"),
    );
    await flushMicrotasks(2);
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
    await flushMicrotasks(2);
    assert.deepEqual(reported, []);
  });
}
