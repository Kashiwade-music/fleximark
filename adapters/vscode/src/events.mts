import * as vscode from "vscode";

interface EventAdapter {
  activateEditor(editor?: vscode.TextEditor): Promise<void>;
  activateDocument(document?: vscode.TextDocument): Promise<void>;
  changeDocument(document: vscode.TextDocument): void;
  closeDocument(document: vscode.TextDocument): void;
  reconfigureWorkspace(workspace: vscode.WorkspaceFolder): Promise<void>;
  removeWorkspace(workspace: vscode.WorkspaceFolder): void;
  report(error: unknown): void;
  selectionChanged(event: vscode.TextEditorSelectionChangeEvent): void;
  viewportChanged(event: vscode.TextEditorVisibleRangesChangeEvent): void;
}

export function registerEditorEvents(
  adapter: EventAdapter,
  registrar: EditorEventRegistrar = {
    workspace: vscode.workspace,
    window: vscode.window,
  },
  isActive: () => boolean = () => true,
  workspaceLifecycle?: {
    added(workspace: vscode.WorkspaceFolder): void;
    removed(workspace: vscode.WorkspaceFolder): void;
    trustGranted(): void;
  },
): vscode.Disposable[] {
  const workspacePolicyWatcher = registrar.workspace.createFileSystemWatcher(
    "**/.fleximark/{config.toml,theme.css,plugins/**}",
  );
  const reconfigureFor = (uri: vscode.Uri) => {
    if (!isActive()) return;
    const workspace = vscode.workspace.getWorkspaceFolder(uri);
    if (workspace)
      void adapter.reconfigureWorkspace(workspace).catch((error: unknown) => {
        if (isActive()) adapter.report(error);
      });
  };
  return [
    registrar.workspace.onDidOpenTextDocument((document) => {
      if (isActive() && document === vscode.window.activeTextEditor?.document)
        void adapter.activateDocument(document).catch((error: unknown) => {
          if (isActive()) adapter.report(error);
        });
    }),
    registrar.window.onDidChangeActiveTextEditor((editor) => {
      if (!isActive()) return;
      void adapter.activateEditor(editor).catch((error: unknown) => {
        if (isActive()) adapter.report(error);
      });
    }),
    registrar.workspace.onDidChangeTextDocument(({ document }) => {
      if (isActive()) adapter.changeDocument(document);
    }),
    registrar.workspace.onDidCloseTextDocument((document) => {
      if (isActive()) adapter.closeDocument(document);
    }),
    registrar.workspace.onDidChangeWorkspaceFolders(({ added, removed }) => {
      if (!isActive()) return;
      for (const workspace of removed) {
        adapter.removeWorkspace(workspace);
        workspaceLifecycle?.removed(workspace);
      }
      for (const workspace of added) workspaceLifecycle?.added(workspace);
    }),
    workspacePolicyWatcher,
    workspacePolicyWatcher.onDidCreate(reconfigureFor),
    workspacePolicyWatcher.onDidChange(reconfigureFor),
    workspacePolicyWatcher.onDidDelete(reconfigureFor),
    registrar.workspace.onDidGrantWorkspaceTrust(() => {
      if (!isActive()) return;
      workspaceLifecycle?.trustGranted();
      for (const workspace of vscode.workspace.workspaceFolders ?? [])
        void adapter.reconfigureWorkspace(workspace).catch((error: unknown) => {
          if (isActive()) adapter.report(error);
        });
    }),
    registrar.window.onDidChangeTextEditorSelection((event) => {
      if (isActive()) adapter.selectionChanged(event);
    }),
    registrar.window.onDidChangeTextEditorVisibleRanges((event) => {
      if (isActive()) adapter.viewportChanged(event);
    }),
  ];
}

export interface EditorEventRegistrar {
  readonly workspace: Pick<
    typeof vscode.workspace,
    | "createFileSystemWatcher"
    | "onDidOpenTextDocument"
    | "onDidChangeTextDocument"
    | "onDidCloseTextDocument"
    | "onDidChangeWorkspaceFolders"
    | "onDidGrantWorkspaceTrust"
  >;
  readonly window: Pick<
    typeof vscode.window,
    | "onDidChangeActiveTextEditor"
    | "onDidChangeTextEditorSelection"
    | "onDidChangeTextEditorVisibleRanges"
  >;
}
