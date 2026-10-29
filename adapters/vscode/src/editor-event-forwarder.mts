import type * as vscode from "vscode";

import type { PreviewController } from "./preview-controller.mjs";
import type { RuntimeManager } from "./runtime-manager.mjs";

/** Forwards document, selection and viewport events without retaining wire state. */
export class EditorEventForwarder {
  constructor(
    private readonly runtime: Pick<
      RuntimeManager,
      | "connection"
      | "runtimeForDocument"
      | "activateDocument"
      | "changeDocument"
      | "closeDocument"
    >,
    private readonly previews: Pick<PreviewController, "echoes" | "follow">,
  ) {}
  async activateEditor(editor?: vscode.TextEditor): Promise<void> {
    await this.runtime.activateDocument(editor?.document);
    await this.previews.follow(editor);
  }
  changeDocument(document: vscode.TextDocument): void {
    this.runtime.changeDocument(document);
  }
  closeDocument(document: vscode.TextDocument): void {
    this.runtime.closeDocument(document);
  }
  selectionChanged(event: vscode.TextEditorSelectionChangeEvent): void {
    const uri = event.textEditor.document.uri.toString();
    const value = event.selections
      .map(
        ({ anchor, active }) =>
          `${anchor.line}:${anchor.character}-${active.line}:${active.character}`,
      )
      .join(",");
    if (
      this.previews.echoes.selection?.uri === uri &&
      this.previews.echoes.selection.value === value
    ) {
      this.previews.echoes.selection = undefined;
      return;
    }
    const state = this.runtime
      .runtimeForDocument(event.textEditor.document)
      ?.documents.get(uri);
    const origin = this.runtime.connection;
    if (!state?.sessionId || !origin) return;
    origin.rpc.notify("fleximark/setSelection", {
      daemonInstanceId: origin.daemonInstanceId,
      documentSessionId: state.sessionId,
      expectedDocumentVersion: event.textEditor.document.version,
      selections: event.selections.map(({ anchor, active }) => ({
        anchor: { line: anchor.line, character: anchor.character },
        active: { line: active.line, character: active.character },
      })),
    });
  }

  viewportChanged(event: vscode.TextEditorVisibleRangesChangeEvent): void {
    const document = event.textEditor.document;
    const uri = document.uri.toString();
    if (this.previews.echoes.viewport?.uri === uri) {
      this.previews.echoes.viewport = undefined;
      return;
    }
    const state = this.runtime.runtimeForDocument(document)?.documents.get(uri);
    const origin = this.runtime.connection;
    if (!state?.sessionId || !origin) return;
    origin.rpc.notify("fleximark/setViewport", {
      daemonInstanceId: origin.daemonInstanceId,
      documentSessionId: state.sessionId,
      expectedDocumentVersion: document.version,
      ranges: event.visibleRanges.map(({ start, end }) => ({
        start: { line: start.line, character: start.character },
        end: { line: end.line, character: end.character },
      })),
    });
  }
}
