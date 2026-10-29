import {
  type LspShowDocumentParams,
  hasContractShape,
} from "@fleximark/contracts/protocol";
import * as vscode from "vscode";

export function isShowDocumentParams(
  value: unknown,
): value is LspShowDocumentParams {
  return hasContractShape("lspShowDocumentParams", value);
}
export async function showDocument(
  params: LspShowDocumentParams,
): Promise<boolean> {
  const uri = vscode.Uri.parse(params.uri);
  if (params.external || uri.scheme === "http" || uri.scheme === "https")
    return vscode.env.openExternal(uri);
  const document = await vscode.workspace.openTextDocument(uri);
  const selection = params.selection
    ? new vscode.Range(
        params.selection.start.line,
        params.selection.start.character,
        params.selection.end.line,
        params.selection.end.character,
      )
    : undefined;
  await vscode.window.showTextDocument(document, {
    preserveFocus: !params.takeFocus,
    preview: false,
    selection,
  });
  return true;
}
