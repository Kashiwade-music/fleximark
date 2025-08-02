import * as path from "path";
import * as vscode from "vscode";

import * as fLibFs from "../../lib/fs/index.mjs";
import * as fLibSettings from "../../lib/settings/index.mjs";

const initializeWorkspace = async (context: vscode.ExtensionContext) => {
  // check if workspace is open
  const workspace = fLibFs.getWorkspaceFoldersOrShowError();
  if (!workspace) {
    return;
  }

  // check if already .vscode dir exists
  const workspacePath = workspace[0].uri.fsPath;
  try {
    await vscode.workspace.fs.stat(
      vscode.Uri.file(path.join(workspacePath, ".vscode")),
    );
    const yesNo = await vscode.window.showInformationMessage(
      vscode.l10n.t(
        "A .vscode directory already exists. Do you want to overwrite it?",
      ),
      "Yes",
      "No",
    );
    if (yesNo !== "Yes") {
      return;
    }
  } catch {
    // .vscode directory does not exist, continue
  }

  await vscode.workspace.fs.createDirectory(
    vscode.Uri.file(path.join(workspacePath, "attachments")),
  );

  // create file and open
  const file = vscode.Uri.file(
    path.join(workspacePath, ".vscode", "settings.json"),
  );
  await vscode.workspace.fs.writeFile(
    file,
    Buffer.from(await fLibSettings.genSettingsJson(context, workspacePath)),
  );

  // create fleximark directory and file. fleximark.json file contains the create date
  const fleximarkDir = vscode.Uri.file(
    path.join(workspacePath, ".fleximark", "fleximark.json"),
  );
  await vscode.workspace.fs.writeFile(
    fleximarkDir,
    Buffer.from(JSON.stringify({ meta: new Date().toISOString() }, null, 2)),
  );

  await vscode.window.showTextDocument(
    await vscode.workspace.openTextDocument(file),
  );
};

export default initializeWorkspace;
