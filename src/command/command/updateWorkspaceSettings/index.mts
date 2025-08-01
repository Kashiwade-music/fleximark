import * as path from "path";
import * as vscode from "vscode";

import * as fLibFs from "../../lib/fs/index.mjs";
import * as fLibSettings from "../../lib/settings/index.mjs";

const updateWorkspaceSettings = async (
  context: vscode.ExtensionContext,
  forceOverwrite = false,
): Promise<void> => {
  const workspaceFolders = fLibFs.getWorkspaceFoldersOrShowError();
  if (!workspaceFolders) {
    return;
  }

  const workspaceRoot = workspaceFolders[0].uri.fsPath;
  const settingsDir = path.join(workspaceRoot, ".vscode");
  const settingsPath = path.join(settingsDir, "settings.json");
  const settingsUri = vscode.Uri.file(settingsPath);

  let shouldProceed = true;

  try {
    // Check if settings file exists
    await vscode.workspace.fs.stat(settingsUri);

    if (!forceOverwrite) {
      const confirmation = await vscode.window.showInformationMessage(
        vscode.l10n.t(
          ".vscode/settings.json already exists. Overwrite it? A backup will be created.",
        ),
        "Yes",
        "No",
      );

      if (confirmation !== "Yes") {
        shouldProceed = false;
      }
    }
  } catch {
    // File does not exist; proceed with creation
  }

  if (!shouldProceed) return;

  // Backup existing settings.json (if present)
  const timestamp = new Date().toISOString().slice(0, 10).replace(/-/g, "");
  const backupFilename = `settings_${timestamp}.json`;
  const backupPath = path.join(settingsDir, backupFilename);
  const backupUri = vscode.Uri.file(backupPath);

  try {
    await vscode.workspace.fs.copy(settingsUri, backupUri, { overwrite: true });
  } catch {
    // Ignore if original settings didn't exist
  }

  // create fleximark directory and file. fleximark.json file contains the create date
  const fleximarkDir = vscode.Uri.file(
    path.join(workspaceRoot, ".fleximark", "fleximark.json"),
  );
  await vscode.workspace.fs.writeFile(
    fleximarkDir,
    Buffer.from(JSON.stringify({ meta: new Date().toISOString() }, null, 2)),
  );

  // Generate and write the new settings content
  const newSettings = await fLibSettings.genSettingsJson(
    context,
    workspaceRoot,
  );
  await vscode.workspace.fs.writeFile(settingsUri, Buffer.from(newSettings));

  // Reveal the updated settings in the editor
  const doc = await vscode.workspace.openTextDocument(settingsUri);
  await vscode.window.showTextDocument(doc);

  vscode.window.showInformationMessage(
    vscode.l10n.t("Updated .vscode/settings.json successfully."),
  );
};

export default updateWorkspaceSettings;
