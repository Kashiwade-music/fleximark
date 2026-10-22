import * as vscode from "vscode";

import type {
  SourceNavigationEvent,
  SourcePosition,
} from "../../../contracts/typescript/protocol.mjs";
import {
  sourcePositionToCharacter,
  sourcePositionWithinLine,
} from "./position.mjs";
import type { PreviewEchoState } from "./preview-coordinator.mjs";
import type { PreviewState, WorkspaceRuntime } from "./runtime-state.mjs";
import { findVisibleSourceEditor } from "./workspace-selection.mjs";

export async function applySourceNavigation(
  runtime: WorkspaceRuntime,
  preview: PreviewState,
  event: SourceNavigationEvent,
  echoes: PreviewEchoState,
  isCurrent: (runtime: WorkspaceRuntime, preview: PreviewState) => boolean,
): Promise<void> {
  const expectedRevision = preview.renderRevision;
  const expectedSession = preview.previewSessionId;
  const expectedOrigin = preview.origin;
  const document = vscode.workspace.textDocuments.find(
    (item) => item.uri.toString() === preview.documentUri,
  );
  if (!document) return;
  const expectedDocumentVersion = document.version;
  const { start, end } = event.sourceRange;
  if (
    !sourcePositionInDocument(document, start) ||
    !sourcePositionInDocument(document, end)
  )
    return;
  const range = new vscode.Range(
    start.line,
    sourcePositionToCharacter(document.lineAt(start.line).text, start),
    end.line,
    sourcePositionToCharacter(document.lineAt(end.line).text, end),
  );
  const editor =
    findVisibleSourceEditor(
      vscode.window.visibleTextEditors,
      preview.documentUri,
      preview.sourceViewColumn,
    ) ??
    (await vscode.window.showTextDocument(document, {
      viewColumn: preview.sourceViewColumn,
      preserveFocus: true,
      preview: false,
    }));
  if (
    !isCurrent(runtime, preview) ||
    preview.previewSessionId !== expectedSession ||
    preview.origin !== expectedOrigin ||
    preview.renderRevision !== expectedRevision ||
    !vscode.workspace.textDocuments.includes(document) ||
    document.version !== expectedDocumentVersion ||
    editor.document !== document
  )
    return;
  const clearEcho = (type: keyof PreviewEchoState, token: object) =>
    setTimeout(() => {
      if (echoes[type] === token) echoes[type] = undefined;
    }, 500);
  const viewportEcho = { uri: preview.documentUri };
  echoes.viewport = viewportEcho;
  if (event.type === "selectSource") {
    const selection = new vscode.Selection(range.start, range.end);
    const selectionEcho = {
      uri: preview.documentUri,
      value: `${selection.anchor.line}:${selection.anchor.character}-${selection.active.line}:${selection.active.character}`,
    };
    echoes.selection = selectionEcho;
    editor.selection = selection;
    clearEcho("selection", selectionEcho);
    editor.revealRange(
      range,
      vscode.TextEditorRevealType.InCenterIfOutsideViewport,
    );
  } else editor.revealRange(range, vscode.TextEditorRevealType.AtTop);
  clearEcho("viewport", viewportEcho);
}

function sourcePositionInDocument(
  document: vscode.TextDocument,
  position: SourcePosition,
): boolean {
  if (position.line < 0 || position.line >= document.lineCount) return false;
  const line = document.lineAt(position.line).text;
  return sourcePositionWithinLine(line, position);
}
