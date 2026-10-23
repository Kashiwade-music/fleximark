import * as vscode from "vscode";

import {
  type TextRange,
  isTextRange,
} from "../../../contracts/typescript/protocol.mjs";

export interface ShowDocumentParams {
  uri: string;
  external?: boolean;
  takeFocus?: boolean;
  selection?: TextRange;
}
export function isShowDocumentParams(
  value: unknown,
): value is ShowDocumentParams {
  if (value === null || typeof value !== "object" || Array.isArray(value))
    return false;
  const p = value as Record<string, unknown>;
  return (
    typeof p.uri === "string" &&
    (p.external === undefined || typeof p.external === "boolean") &&
    (p.takeFocus === undefined || typeof p.takeFocus === "boolean") &&
    (p.selection === undefined || isTextRange(p.selection))
  );
}
export async function showDocument(
  params: ShowDocumentParams,
): Promise<boolean> {
  const uri = vscode.Uri.parse(params.uri);
  if (params.external || uri.scheme === "http" || uri.scheme === "https")
    return vscode.env.openExternal(uri);
  const document = await vscode.workspace.openTextDocument(uri);
  const selection =
    params.selection &&
    new vscode.Range(
      params.selection.start.line,
      params.selection.start.character,
      params.selection.end.line,
      params.selection.end.character,
    );
  await vscode.window.showTextDocument(document, {
    preserveFocus: !params.takeFocus,
    preview: false,
    selection,
  });
  return true;
}
