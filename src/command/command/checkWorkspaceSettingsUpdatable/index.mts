import * as path from "path";
import * as vscode from "vscode";

import * as fLibFs from "../../lib/fs/index.mjs";
import * as fLibJsonc from "../../lib/jsonc/index.mjs";
import * as fLibSettings from "../../lib/settings/index.mjs";

const checkWorkspaceSettingsUpdatable = async (): Promise<boolean> => {
  const workspaceFolders = fLibFs.getWorkspaceFoldersOrShowError();
  if (!workspaceFolders) {
    return false;
  }

  const workspacePath = workspaceFolders[0].uri.fsPath;
  const settingsFilePath = path.join(workspacePath, ".vscode", "settings.json");
  const settingsFileUri = vscode.Uri.file(settingsFilePath);

  try {
    // Check if settings.json exists
    await vscode.workspace.fs.stat(settingsFileUri);

    const rawContent = await vscode.workspace.fs.readFile(settingsFileUri);
    const settings = fLibJsonc.parse(rawContent.toString());

    const version = settings["fleximark.settingsVersion"];

    if (typeof version !== "number") {
      return true; // No version specified, eligible for update
    }

    if (version === fLibSettings.CURRENT_SETTINGS_VERSION) {
      vscode.window.showInformationMessage(
        vscode.l10n.t("The .vscode/settings.json file is already up to date."),
      );
      return false;
    }

    if (version > fLibSettings.CURRENT_SETTINGS_VERSION) {
      vscode.window.showWarningMessage(
        vscode.l10n.t(
          "The .vscode/settings.json file is newer than the extension version. Please update the extension.",
        ),
      );
      return false;
    }

    // settings.json is outdated
    const userResponse = await vscode.window.showInformationMessage(
      vscode.l10n.t(
        "The .vscode/settings.json file is outdated. Do you want to update it?",
      ),
      "Yes",
      "No",
    );

    return userResponse === "Yes";
  } catch {
    // settings.json does not exist
    vscode.window.showInformationMessage(
      vscode.l10n.t("No .vscode/settings.json file found."),
    );
    return false;
  }
};

export default checkWorkspaceSettingsUpdatable;
