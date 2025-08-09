import * as path from "path";
import * as vscode from "vscode";

import * as fLibFs from "../../lib/fs/index.mjs";
import * as fLibSettings from "../../lib/settings/index.mjs";

const updateWorkspaceSettings = async (
  context: vscode.ExtensionContext,
  forceOverwrite = false,
): Promise<void> => {
  const workspaceFolders = fLibFs.getWorkspaceFoldersOrShowError();
  if (!workspaceFolders) return;

  const workspaceRoot = workspaceFolders[0].uri.fsPath;
  const settingsDir = path.join(workspaceRoot, ".vscode");
  const settingsPath = path.join(settingsDir, "settings.json");
  const settingsUri = vscode.Uri.file(settingsPath);

  const proceed = await fLibFs.confirmOverwriteAndBackupFile(
    settingsUri,
    forceOverwrite,
  );
  if (!proceed) return;

  const fleximarkUri = vscode.Uri.file(
    path.join(workspaceRoot, ".fleximark", "fleximark.json"),
  );
  await vscode.workspace.fs.writeFile(
    fleximarkUri,
    Buffer.from(JSON.stringify({ meta: new Date().toISOString() }, null, 2)),
  );

  const newSettings = await fLibSettings.genSettingsJson(
    context,
    workspaceRoot,
  );
  await vscode.workspace.fs.writeFile(settingsUri, Buffer.from(newSettings));

  const doc = await vscode.workspace.openTextDocument(settingsUri);
  await vscode.window.showTextDocument(doc);

  vscode.window.showInformationMessage(
    vscode.l10n.t("Updated .vscode/settings.json successfully."),
  );
};

export default updateWorkspaceSettings;
