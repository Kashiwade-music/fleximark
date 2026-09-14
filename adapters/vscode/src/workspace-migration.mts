import * as vscode from "vscode";

import {
  type LegacyWorkspaceSettings,
  type LegacyWorkspaceState,
  WorkspaceMigrationController,
  createMigratedConfig,
} from "./workspace-migration-policy.mjs";

const DEFAULT_THEME =
  "/* FlexiMark workspace theme */\n:root { color-scheme: light dark; }\n";

export class LegacyWorkspaceMigrator {
  readonly #controller =
    new WorkspaceMigrationController<vscode.WorkspaceFolder>({
      key: (workspace) => workspace.uri.toString(),
      detect: (workspace) => detectLegacyWorkspace(workspace),
      confirm: (workspace, state) => confirmMigration(workspace, state),
      migrate: (workspace, state) => migrateWorkspace(workspace, state),
      completed: (workspace, result) =>
        showMigrationCompleted(workspace, result),
    });

  offer(workspace: vscode.WorkspaceFolder): Promise<void> {
    return this.#controller.offer(workspace);
  }

  forget(workspace: vscode.WorkspaceFolder): void {
    this.#controller.forget(workspace);
  }
}

export async function detectLegacyWorkspace(
  workspace: vscode.WorkspaceFolder,
): Promise<LegacyWorkspaceState | undefined> {
  const control = vscode.Uri.joinPath(workspace.uri, ".fleximark");
  const marker = vscode.Uri.joinPath(control, "fleximark.json");
  const config = vscode.Uri.joinPath(control, "config.toml");
  if (
    !(await directoryExists(control)) ||
    !(await regularFileExists(marker)) ||
    (await pathExists(config))
  )
    return;
  return {
    hasLegacyTheme: await regularFileExists(
      vscode.Uri.joinPath(control, "fleximark.css"),
    ),
    hasLegacyPlugin: await regularFileExists(
      vscode.Uri.joinPath(control, "parserPlugin.js"),
    ),
    hasAttachments: await directoryExists(
      vscode.Uri.joinPath(workspace.uri, "attachments"),
    ),
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
): Promise<{ legacyPluginRetained: boolean }> {
  const control = vscode.Uri.joinPath(workspace.uri, ".fleximark");
  const marker = vscode.Uri.joinPath(control, "fleximark.json");
  if (!(await directoryExists(control)) || !(await regularFileExists(marker)))
    throw new Error("The legacy FlexiMark workspace changed during migration");
  const config = vscode.Uri.joinPath(control, "config.toml");
  if (await pathExists(config)) return { legacyPluginRetained: false };

  const theme = vscode.Uri.joinPath(control, "theme.css");
  if (!(await pathExists(theme))) {
    const contents = state.hasLegacyTheme
      ? await vscode.workspace.fs.readFile(
          vscode.Uri.joinPath(control, "fleximark.css"),
        )
      : Buffer.from(DEFAULT_THEME);
    await vscode.workspace.fs.writeFile(theme, contents);
  }

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

  // config.toml is written last because its presence marks a completed migration.
  if (await pathExists(config)) return { legacyPluginRetained: false };
  await vscode.workspace.fs.writeFile(
    config,
    Buffer.from(createMigratedConfig(state.settings, state.hasAttachments)),
  );
  return { legacyPluginRetained: state.hasLegacyPlugin };
}

async function showMigrationCompleted(
  workspace: vscode.WorkspaceFolder,
  result: { legacyPluginRetained: boolean },
): Promise<void> {
  const message = result.legacyPluginRetained
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

async function regularFileExists(uri: vscode.Uri): Promise<boolean> {
  const stat = await statOrUndefined(uri);
  return (
    stat !== undefined &&
    (stat.type & vscode.FileType.File) !== 0 &&
    (stat.type & vscode.FileType.SymbolicLink) === 0
  );
}

async function directoryExists(uri: vscode.Uri): Promise<boolean> {
  const stat = await statOrUndefined(uri);
  return (
    stat !== undefined &&
    (stat.type & vscode.FileType.Directory) !== 0 &&
    (stat.type & vscode.FileType.SymbolicLink) === 0
  );
}

async function pathExists(uri: vscode.Uri): Promise<boolean> {
  return (await statOrUndefined(uri)) !== undefined;
}

async function statOrUndefined(
  uri: vscode.Uri,
): Promise<vscode.FileStat | undefined> {
  try {
    return await vscode.workspace.fs.stat(uri);
  } catch (error) {
    if (
      error instanceof vscode.FileSystemError &&
      error.code === "FileNotFound"
    )
      return;
    throw error;
  }
}
