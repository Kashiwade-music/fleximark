import {
  COMMAND_DESCRIPTORS,
  WORKSPACE_COMMAND_KIND,
  type WorkspaceCommandRequest,
} from "@fleximark/contracts/protocol";
import * as vscode from "vscode";

import {
  buildWorkspaceOperation,
  confirmMigrationPlan,
  executeCreateNote,
  executeMigration,
  openCommandResult,
  showMigrationPlanStatus,
} from "./commands.mjs";
import type { RuntimeManager } from "./runtime-manager.mjs";
import { selectWorkspaceUri } from "./workspace-selection.mjs";

/** Executes typed workspace commands and coordinates their editor UI. */
export class CommandService {
  constructor(private readonly runtime: RuntimeManager) {}
  private showNoDocument(): void {
    void vscode.window.showInformationMessage(
      vscode.l10n.t("Open a Markdown document first."),
    );
  }
  private showNoWorkspace(): void {
    void vscode.window.showInformationMessage(
      vscode.l10n.t("Open the Markdown document inside a workspace first."),
    );
  }
  async execute(command: string): Promise<void> {
    const editor = vscode.window.activeTextEditor;
    const document = editor?.document;
    if (command === "previewAt") {
      if (document?.languageId !== "markdown") {
        this.showNoDocument();
        return;
      }
    }
    const descriptor = COMMAND_DESCRIPTORS.find(
      (entry) =>
        entry.name === command ||
        (command === "workspace" &&
          entry.kind === WORKSPACE_COMMAND_KIND.openWorkspace) ||
        (command === "previewAt" &&
          entry.kind === WORKSPACE_COMMAND_KIND.openPreview),
    );
    if (!descriptor)
      throw new Error(
        vscode.l10n.t("Unknown workspace operation: {0}", command),
      );
    const folders = vscode.workspace.workspaceFolders ?? [];
    const activeWorkspace = document
      ? vscode.workspace.getWorkspaceFolder(document.uri)
      : undefined;
    const workspaceUri = await selectWorkspaceUri(
      activeWorkspace?.uri.toString(),
      folders.map((folder) => ({
        label: folder.name,
        uri: folder.uri.toString(),
      })),
      vscode.window.showQuickPick,
    );
    if (descriptor.requiresWorkspace && !workspaceUri) {
      this.showNoWorkspace();
      return;
    }
    if (descriptor.requiresDocument && document?.languageId !== "markdown") {
      this.showNoDocument();
      return;
    }
    const workspace = folders.find(
      (folder) => folder.uri.toString() === workspaceUri,
    );
    const runtime = await this.runtime.start(workspace);
    if (!runtime) return;
    if (document?.languageId === "markdown")
      await this.runtime.syncDocument(document);
    const state = document && runtime.documents.get(document.uri.toString());
    const origin = this.runtime.connection;
    if (!origin) return;
    const operation = buildWorkspaceOperation(
      descriptor.kind,
      editor && document
        ? Buffer.byteLength(
            document
              .getText()
              .slice(0, document.offsetAt(editor.selection.active)),
            "utf8",
          )
        : undefined,
    );
    const params: WorkspaceCommandRequest = {
      daemonInstanceId: origin.daemonInstanceId,
      context: {
        ...(workspaceUri ? { workspaceUri } : {}),
        ...(state?.sessionId && document
          ? {
              document: {
                documentSessionId: state.sessionId,
                expectedDocumentVersion: document.version,
              },
            }
          : {}),
      },
      operation,
    };
    const execute = (request: WorkspaceCommandRequest) =>
      origin.rpc.request("fleximark/executeCommand", request);
    const result =
      descriptor.kind === WORKSPACE_COMMAND_KIND.createNote
        ? await executeCreateNote(
            params,
            async (request) => {
              const options = await execute(request);
              if (options.kind !== "noteOptions")
                throw new Error(vscode.l10n.t("Invalid note options response"));
              return options.options;
            },
            (items, options) => vscode.window.showQuickPick(items, options),
            (items, options) => vscode.window.showQuickPick(items, options),
            (options) => vscode.window.showInputBox(options),
            execute,
          )
        : descriptor.kind === WORKSPACE_COMMAND_KIND.planMigration
          ? await executeMigration(
              params,
              execute,
              confirmMigrationPlan,
              () => !origin.signal.aborted,
            )
          : await execute(params);
    if (!result || !!origin.signal.aborted) return;
    if (result.kind === "migrationPlanned")
      await showMigrationPlanStatus(result.plan);
    if (result.kind === "workspaceOpened")
      await vscode.env.openExternal(vscode.Uri.parse(result.url));
    if (result.kind === "previewOpened" && result.preview.url)
      await vscode.env.openExternal(vscode.Uri.parse(result.preview.url));
    await openCommandResult(
      params,
      result,
      async (uri) => {
        await vscode.window.showTextDocument(vscode.Uri.parse(uri));
      },
      execute,
    );
    if (result.message) {
      const show =
        result.message.level === "error"
          ? vscode.window.showErrorMessage
          : result.message.level === "warning"
            ? vscode.window.showWarningMessage
            : vscode.window.showInformationMessage;
      await show(result.message.text);
    }
  }
}
