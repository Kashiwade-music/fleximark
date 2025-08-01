import * as path from "path";
import * as vscode from "vscode";

import * as fLibJsonc from "../command/lib/jsonc/index.mjs";

const CURRENT_SETTINGS_VERSION = 1;

const checkWorkspaceSettingsUpdatable = async (): Promise<boolean> => {
  const ret = await checkWorkspaceSettingsUpdatableImpl();
  if (ret) {
    // ask
    const userResponse = await vscode.window.showInformationMessage(
      vscode.l10n.t(
        "The .vscode/settings.json file is outdated. Do you want to update it?",
      ),
      "Yes",
      "No",
    );
    return userResponse === "Yes";
  } else {
    return false;
  }
};

const checkWorkspaceSettingsUpdatableImpl = async (): Promise<boolean> => {
  const workspaceFolders = vscode.workspace.workspaceFolders;

  if (!workspaceFolders?.length) {
    vscode.window.showErrorMessage(
      vscode.l10n.t("Please open a workspace before initializing."),
    );
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

    if (version === CURRENT_SETTINGS_VERSION) {
      vscode.window.showInformationMessage(
        vscode.l10n.t("The .vscode/settings.json file is already up to date."),
      );
      return false;
    }

    if (version > CURRENT_SETTINGS_VERSION) {
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
      { modal: true },
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
