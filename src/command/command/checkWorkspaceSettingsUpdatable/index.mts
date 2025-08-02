import * as vscode from "vscode";

import process from "./process.mjs";

const checkWorkspaceSettingsUpdatable = async (): Promise<boolean> => {
  const ret = await process();
  if (ret) {
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

export default checkWorkspaceSettingsUpdatable;
