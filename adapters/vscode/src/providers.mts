import {
  type LspMethod,
  type LspSemanticTokens,
  SEMANTIC_TOKEN_MODIFIERS,
  SEMANTIC_TOKEN_TYPES,
  type TextPosition,
  type TextRange,
} from "@fleximark/contracts/protocol";
import * as vscode from "vscode";

interface ProviderAdapter {
  requestLanguageFeature<T>(
    method: LspMethod,
    document: vscode.TextDocument,
    params?: object,
  ): Promise<T | undefined>;
}

function toWirePosition(position: vscode.Position): TextPosition {
  return { line: position.line, character: position.character };
}

function toVscodeRange(range: TextRange): vscode.Range {
  return new vscode.Range(
    range.start.line,
    range.start.character,
    range.end.line,
    range.end.character,
  );
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
                kind: number;
                detail: string;
                filterText: string;
                insertTextFormat: number;
                textEdit: {
                  range: TextRange;
                  newText: string;
                };
              }[];
            }>("textDocument/completion", document, {
              position: toWirePosition(position),
            }),
          );
          return (result?.items ?? []).map((item) => {
            const completion = new vscode.CompletionItem(
              item.label,
              item.kind as vscode.CompletionItemKind,
            );
            completion.detail = item.detail;
            completion.filterText = item.filterText;
            completion.range = toVscodeRange(item.textEdit.range);
            completion.insertText =
              item.insertTextFormat === 2
                ? new vscode.SnippetString(item.textEdit.newText)
                : item.textEdit.newText;
            return completion;
          });
        },
      },
      ":",
      "`",
      ">",
    ),
    registrar.registerDocumentSemanticTokensProvider(
      { language: "markdown" },
      {
        async provideDocumentSemanticTokens(document) {
          const result = await request(() =>
            adapter.requestLanguageFeature<LspSemanticTokens>(
              "textDocument/semanticTokens/full",
              document,
            ),
          );
          return new vscode.SemanticTokens(
            Uint32Array.from(result?.data ?? []),
          );
        },
      },
      new vscode.SemanticTokensLegend(
        [...SEMANTIC_TOKEN_TYPES],
        [...SEMANTIC_TOKEN_MODIFIERS],
      ),
    ),
    registrar.registerHoverProvider(
      { language: "markdown" },
      {
        async provideHover(document, position) {
          const result = await request(() =>
            adapter.requestLanguageFeature<{
              contents: { value: string };
            } | null>("textDocument/hover", document, {
              position: toWirePosition(position),
            }),
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
                range: TextRange;
                selectionRange: TextRange;
              }[]
            >("textDocument/documentSymbol", document),
          );
          return (result ?? []).map(
            (item) =>
              new vscode.DocumentSymbol(
                item.name,
                "",
                item.kind as vscode.SymbolKind,
                toVscodeRange(item.range),
                toVscodeRange(item.selectionRange),
              ),
          );
        },
      },
    ),
  ];
}

export type ProviderRegistrar = Pick<
  typeof vscode.languages,
  | "registerCompletionItemProvider"
  | "registerDocumentSemanticTokensProvider"
  | "registerHoverProvider"
  | "registerDocumentSymbolProvider"
>;
