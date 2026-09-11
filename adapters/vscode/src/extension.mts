import * as vscode from "vscode";

import { type AdapterRecoveryState, FlexiMarkAdapter } from "./adapter.mjs";

let adapter: FlexiMarkAdapter | undefined;

export interface FlexiMarkTestApi {
  crashDaemon(): void;
  recoveryState(): AdapterRecoveryState;
}

export function activate(
  context: vscode.ExtensionContext,
): FlexiMarkTestApi | undefined {
  adapter = new FlexiMarkAdapter(context);
  context.subscriptions.push(adapter);
  const run = (action: () => void | Promise<void>) => () =>
    Promise.resolve(action()).catch((error: unknown) => {
      adapter?.report(error);
      throw error;
    });
  const workspacePolicyWatcher = vscode.workspace.createFileSystemWatcher(
    "**/.fleximark/{config.toml,theme.css,plugins/**}",
  );
  const reconfigureFor = (uri: vscode.Uri) => {
    const workspace = vscode.workspace.getWorkspaceFolder(uri);
    if (workspace)
      void adapter
        ?.reconfigureWorkspace(workspace)
        .catch((error: unknown) => adapter?.report(error));
  };

  context.subscriptions.push(
    vscode.languages.registerCompletionItemProvider(
      { language: "markdown" },
      {
        async provideCompletionItems(document, position) {
          const result = await adapter?.requestLanguageFeature<{
            items: {
              label: string;
              insertText?: string;
              insertTextFormat?: number;
            }[];
          }>("textDocument/completion", document, { position });
          return (result?.items ?? []).map((item) => {
            const completion = new vscode.CompletionItem(item.label);
            if (item.insertText)
              completion.insertText =
                item.insertTextFormat === 2
                  ? new vscode.SnippetString(item.insertText)
                  : item.insertText;
            return completion;
          });
        },
      },
      ":",
      "`",
    ),
    vscode.languages.registerHoverProvider(
      { language: "markdown" },
      {
        async provideHover(document, position) {
          const result = await adapter?.requestLanguageFeature<{
            contents: { value: string };
          } | null>("textDocument/hover", document, { position });
          return result
            ? new vscode.Hover(new vscode.MarkdownString(result.contents.value))
            : null;
        },
      },
    ),
    vscode.languages.registerDocumentSymbolProvider(
      { language: "markdown" },
      {
        async provideDocumentSymbols(document) {
          const result = await adapter?.requestLanguageFeature<
            {
              name: string;
              kind: number;
              range: { start: vscode.Position; end: vscode.Position };
              selectionRange: { start: vscode.Position; end: vscode.Position };
            }[]
          >("textDocument/documentSymbol", document);
          return (result ?? []).map(
            (item) =>
              new vscode.DocumentSymbol(
                item.name,
                "",
                item.kind as vscode.SymbolKind,
                new vscode.Range(item.range.start, item.range.end),
                new vscode.Range(
                  item.selectionRange.start,
                  item.selectionRange.end,
                ),
              ),
          );
        },
      },
    ),
    vscode.languages.registerCodeActionsProvider(
      { language: "markdown" },
      {
        async provideCodeActions(document, range, actionContext) {
          const diagnostics = actionContext.diagnostics.map((diagnostic) => ({
            range: diagnostic.range,
            message: diagnostic.message,
            code: diagnostic.code,
            source: diagnostic.source,
            data: (diagnostic as vscode.Diagnostic & { data?: unknown }).data,
          }));
          const result = await adapter?.requestLanguageFeature<
            {
              title: string;
              kind: string;
              edit?: {
                changes?: Record<
                  string,
                  {
                    range: { start: vscode.Position; end: vscode.Position };
                    newText: string;
                  }[]
                >;
              };
            }[]
          >("textDocument/codeAction", document, {
            range,
            context: { diagnostics },
          });
          return (result ?? []).map((item) => {
            const action = new vscode.CodeAction(
              item.title,
              vscode.CodeActionKind.QuickFix,
            );
            if (item.edit?.changes) {
              const edit = new vscode.WorkspaceEdit();
              for (const [uri, edits] of Object.entries(item.edit.changes))
                for (const textEdit of edits)
                  edit.replace(
                    vscode.Uri.parse(uri),
                    new vscode.Range(textEdit.range.start, textEdit.range.end),
                    textEdit.newText,
                  );
              action.edit = edit;
            }
            return action;
          });
        },
      },
      { providedCodeActionKinds: [vscode.CodeActionKind.QuickFix] },
    ),
    vscode.commands.registerCommand(
      "fleximark.previewMarkdown",
      run(() => {
        const target = vscode.workspace
          .getConfiguration("fleximark")
          .get<"embeddedHtml" | "externalBrowser">(
            "previewTarget",
            "embeddedHtml",
          );
        return adapter?.openPreview(target);
      }),
    ),
    vscode.commands.registerCommand(
      "fleximark.previewMarkdownOnVscode",
      run(() => adapter?.openPreview("embeddedHtml")),
    ),
    vscode.commands.registerCommand(
      "fleximark.previewMarkdownOnBrowser",
      run(() => adapter?.openPreview("externalBrowser")),
    ),
    vscode.commands.registerCommand("fleximark.forceReloadPreview", () =>
      adapter?.forceReload(),
    ),
    ...[
      "exportHtml",
      "createNote",
      "initializeWorkspace",
      "collectAdmonitions",
      "editTheme",
    ].map((command) =>
      vscode.commands.registerCommand(
        `fleximark.${command}`,
        run(() => adapter?.execute(command)),
      ),
    ),
    vscode.workspace.onDidOpenTextDocument((document) => {
      if (document === vscode.window.activeTextEditor?.document)
        void adapter
          ?.activateDocument(document)
          .catch((error: unknown) => adapter?.report(error));
    }),
    vscode.window.onDidChangeActiveTextEditor((editor) => {
      void adapter
        ?.activateDocument(editor?.document)
        .catch((error: unknown) => adapter?.report(error));
    }),
    vscode.workspace.onDidChangeTextDocument(({ document }) =>
      adapter?.changeDocument(document),
    ),
    vscode.workspace.onDidCloseTextDocument((document) =>
      adapter?.closeDocument(document),
    ),
    vscode.workspace.onDidChangeWorkspaceFolders(({ removed }) => {
      for (const workspace of removed) adapter?.removeWorkspace(workspace);
    }),
    workspacePolicyWatcher,
    workspacePolicyWatcher.onDidCreate(reconfigureFor),
    workspacePolicyWatcher.onDidChange(reconfigureFor),
    workspacePolicyWatcher.onDidDelete(reconfigureFor),
    vscode.workspace.onDidGrantWorkspaceTrust(() => {
      for (const workspace of vscode.workspace.workspaceFolders ?? [])
        void adapter
          ?.reconfigureWorkspace(workspace)
          .catch((error: unknown) => adapter?.report(error));
    }),
    vscode.window.onDidChangeTextEditorSelection((event) =>
      adapter?.selectionChanged(event),
    ),
    vscode.window.onDidChangeTextEditorVisibleRanges((event) =>
      adapter?.viewportChanged(event),
    ),
  );

  void adapter
    .activateDocument(vscode.window.activeTextEditor?.document)
    .then(() => {
      const config = vscode.workspace.getConfiguration("fleximark");
      if (
        config.get<boolean>("autoOpenPreview", false) &&
        vscode.window.activeTextEditor?.document.languageId === "markdown"
      ) {
        return adapter?.openPreview(
          config.get<"embeddedHtml" | "externalBrowser">(
            "previewTarget",
            "embeddedHtml",
          ),
        );
      }
    })
    .catch((error: unknown) => adapter?.report(error));

  if (context.extensionMode === vscode.ExtensionMode.Test) {
    return {
      crashDaemon: () => adapter?.crashDaemonForTest(),
      recoveryState: () => {
        if (!adapter) throw new Error("FlexiMark adapter is unavailable");
        return adapter.recoveryStateForTest();
      },
    };
  }
}

export function deactivate(): void {
  adapter?.dispose();
  adapter = undefined;
}
