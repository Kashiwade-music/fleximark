import {
  type LspMethod,
  type PreviewTarget,
  WORKSPACE_COMMAND_KIND,
  commandDescriptor,
} from "@fleximark/contracts/protocol";
import type * as vscode from "vscode";

import {
  type AdapterServices,
  createAdapterServices,
} from "./adapter-services.mjs";
import type { DocumentRuntime } from "./runtime-state.mjs";

/** Exposes the editor integration while delegating each state to its owner. */
export class FlexiMarkAdapter implements vscode.Disposable {
  #disposed = false;
  constructor(
    context: vscode.ExtensionContext,
    private readonly services: AdapterServices = createAdapterServices(context),
  ) {}
  start(
    workspace?: vscode.WorkspaceFolder,
  ): Promise<DocumentRuntime | undefined> {
    return this.services.runtime.start(workspace);
  }
  activateDocument(document?: vscode.TextDocument): Promise<void> {
    return this.services.runtime.activateDocument(document);
  }
  activateEditor(editor?: vscode.TextEditor): Promise<void> {
    return this.services.editor.activateEditor(editor);
  }
  syncDocument(document: vscode.TextDocument): Promise<void> {
    return this.services.runtime.syncDocument(document);
  }
  changeDocument(document: vscode.TextDocument): void {
    this.services.editor.changeDocument(document);
  }
  closeDocument(document: vscode.TextDocument): void {
    this.services.editor.closeDocument(document);
  }
  selectionChanged(event: vscode.TextEditorSelectionChangeEvent): void {
    this.services.editor.selectionChanged(event);
  }
  viewportChanged(event: vscode.TextEditorVisibleRangesChangeEvent): void {
    this.services.editor.viewportChanged(event);
  }
  requestLanguageFeature<T>(
    method: LspMethod,
    document: vscode.TextDocument,
    params: object = {},
  ): Promise<T | undefined> {
    return this.services.runtime.requestLanguageFeature<T>(
      method,
      document,
      params,
    );
  }
  async openPreview(target: PreviewTarget): Promise<void> {
    await this.services.previews.open(target);
  }
  execute(command: string): Promise<void> {
    return this.services.commands.execute(command);
  }
  migrateWorkspace(): Promise<void> {
    return this.execute(
      commandDescriptor(WORKSPACE_COMMAND_KIND.planMigration).name,
    );
  }
  forceReload(): Promise<void> {
    return this.services.previews.refresh();
  }
  reconfigureWorkspace(workspace: vscode.WorkspaceFolder): Promise<void> {
    return this.services.runtime.reconfigureWorkspace(workspace);
  }
  removeWorkspace(workspace: vscode.WorkspaceFolder): void {
    this.services.runtime.removeWorkspace(workspace);
  }
  report(error: unknown): void {
    this.services.logger.report(error);
  }
  dispose(): void {
    if (this.#disposed) return;
    this.#disposed = true;
    this.services.previews.dispose();
    this.services.runtime.dispose();
    this.services.router.dispose();
    this.services.logger.dispose();
  }
}
