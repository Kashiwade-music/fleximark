import * as vscode from "vscode";

import type { CommandResult } from "./protocol.mjs";
import {
  type LegacyWorkspaceSettings,
  type LegacyWorkspaceState,
  WorkspaceMigrationController,
} from "./workspace-migration-policy.mjs";

type MigrationCommand = "inspectLegacyWorkspace" | "migrateWorkspace";
export type ExecuteMigrationCommand = (
  workspace: vscode.WorkspaceFolder,
  command: MigrationCommand,
  args?: readonly string[],
) => Promise<CommandResult>;

export class LegacyWorkspaceMigrator {
  readonly #controller: WorkspaceMigrationController<vscode.WorkspaceFolder>;

  constructor(execute: ExecuteMigrationCommand) {
    this.#controller = new WorkspaceMigrationController({
      key: (workspace) => workspace.uri.toString(),
      detect: (workspace) => detectLegacyWorkspace(workspace, execute),
      confirm: (workspace, state) => confirmMigration(workspace, state),
      migrate: (workspace, state) =>
        migrateWorkspace(workspace, state, execute),
      completed: (workspace, state) => showMigrationCompleted(workspace, state),
    });
  }

  offer(workspace: vscode.WorkspaceFolder): Promise<void> {
    return this.#controller.offer(workspace);
  }

  forget(workspace: vscode.WorkspaceFolder): void {
    this.#controller.forget(workspace);
  }
}

export async function detectLegacyWorkspace(
  workspace: vscode.WorkspaceFolder,
  execute: ExecuteMigrationCommand,
): Promise<LegacyWorkspaceState | undefined> {
  const { data } = await execute(workspace, "inspectLegacyWorkspace");
  if (data === undefined) return;
  if (data !== "legacy" && data !== "legacy-plugin")
    throw new Error("Invalid legacy workspace inspection result");
  return {
    hasLegacyPlugin: data === "legacy-plugin",
    settings: readLegacySettings(workspace),
  };
}

function readLegacySettings(
  workspace: vscode.WorkspaceFolder,
): LegacyWorkspaceSettings {
  const config = vscode.workspace.getConfiguration("fleximark", workspace.uri);
  return {
    defaultPreviewMode: config.get("defaultPreviewMode"),
    noteCategories: config.get("noteCategories"),
    noteFileNamePrefix: config.get("noteFileNamePrefix"),
    noteFileNameSuffix: config.get("noteFileNameSuffix"),
    noteTemplates: config.get("noteTemplates"),
  };
}

async function confirmMigration(
  workspace: vscode.WorkspaceFolder,
  state: LegacyWorkspaceState,
): Promise<boolean> {
  const migrate = vscode.l10n.t("Migrate");
  const detail = state.hasLegacyPlugin
    ? vscode.l10n.t(
        "Your note settings and workspace theme will be migrated. The legacy JavaScript plugin will be kept, but it cannot run in this version.",
      )
    : vscode.l10n.t(
        "Your note settings and workspace theme will be migrated. Legacy files will be kept as a backup.",
      );
  const choice = await vscode.window.showInformationMessage(
    vscode.l10n.t(
      "The FlexiMark workspace “{0}” uses the legacy format. Migrate it to the current format?",
      workspace.name,
    ),
    { modal: true, detail },
    migrate,
  );
  return choice === migrate;
}

export async function migrateWorkspace(
  workspace: vscode.WorkspaceFolder,
  state: LegacyWorkspaceState,
  execute: ExecuteMigrationCommand,
): Promise<void> {
  if (
    state.settings.defaultPreviewMode === "vscode" ||
    state.settings.defaultPreviewMode === "browser"
  )
    await vscode.workspace
      .getConfiguration("fleximark", workspace.uri)
      .update(
        "previewTarget",
        state.settings.defaultPreviewMode === "browser"
          ? "externalBrowser"
          : "embeddedHtml",
        vscode.ConfigurationTarget.WorkspaceFolder,
      );
  await execute(workspace, "migrateWorkspace", [
    JSON.stringify(state.settings),
  ]);
}

async function showMigrationCompleted(
  workspace: vscode.WorkspaceFolder,
  state: LegacyWorkspaceState,
): Promise<void> {
  const message = state.hasLegacyPlugin
    ? vscode.l10n.t(
        "Migrated FlexiMark workspace “{0}”. Its legacy JavaScript plugin was retained but is disabled.",
        workspace.name,
      )
    : vscode.l10n.t(
        "Migrated FlexiMark workspace “{0}” to the current format.",
        workspace.name,
      );
  await vscode.window.showInformationMessage(message);
}
