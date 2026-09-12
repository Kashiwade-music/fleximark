import * as vscode from "vscode";

import type { LspMethod } from "./protocol.mjs";

interface ProviderAdapter {
  requestLanguageFeature<T>(
    method: LspMethod,
    document: vscode.TextDocument,
    params?: object,
  ): Promise<T | undefined>;
}

export function registerProviders(
  adapter: ProviderAdapter,
  registrar: ProviderRegistrar = vscode.languages,
  isActive: () => boolean = () => true,
): vscode.Disposable[] {
  const request = async <T,>(
    action: () => Promise<T>,
  ): Promise<T | undefined> => {
    if (!isActive()) return;
    return action();
  };
  return [
    registrar.registerCompletionItemProvider(
      { language: "markdown" },
      {
        async provideCompletionItems(document, position) {
          const result = await request(() =>
            adapter.requestLanguageFeature<{
              items: {
                label: string;
                insertText?: string;
                insertTextFormat?: number;
              }[];
            }>("textDocument/completion", document, { position }),
          );
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
    registrar.registerHoverProvider(
      { language: "markdown" },
      {
        async provideHover(document, position) {
          const result = await request(() =>
            adapter.requestLanguageFeature<{
              contents: { value: string };
            } | null>("textDocument/hover", document, { position }),
          );
          return result
            ? new vscode.Hover(new vscode.MarkdownString(result.contents.value))
            : null;
        },
      },
    ),
    registrar.registerDocumentSymbolProvider(
      { language: "markdown" },
      {
        async provideDocumentSymbols(document) {
          const result = await request(() =>
            adapter.requestLanguageFeature<
              {
                name: string;
                kind: number;
                range: { start: vscode.Position; end: vscode.Position };
                selectionRange: {
                  start: vscode.Position;
                  end: vscode.Position;
                };
              }[]
            >("textDocument/documentSymbol", document),
          );
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
    registrar.registerCodeActionsProvider(
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
          const result = await request(() =>
            adapter.requestLanguageFeature<
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
            }),
          );
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
  ];
}

export type ProviderRegistrar = Pick<
  typeof vscode.languages,
  | "registerCompletionItemProvider"
  | "registerHoverProvider"
  | "registerDocumentSymbolProvider"
  | "registerCodeActionsProvider"
>;
