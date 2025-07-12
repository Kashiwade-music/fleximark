import * as vscode from "vscode";
import * as path from "path";
import * as fs from "fs";

const getSettingsJsonString = async (context: vscode.ExtensionContext) => {
  return await fs.promises.readFile(
    context.asAbsolutePath(
      path.join("dist", "media", "workspaceSettingsJsonTemplate.jsonc")
    ),
    "utf8"
  );
};

const initializeWorkspace = async (context: vscode.ExtensionContext) => {
  // check if workspace is open
  const workspace = vscode.workspace.workspaceFolders;
  if (!workspace) {
    vscode.window.showErrorMessage(
      vscode.l10n.t("Please open a workspace before initializing.")
    );
    return;
  }

  // check if already .vscode dir exists
  const workspacePath = workspace[0].uri.fsPath;
  try {
    await vscode.workspace.fs.stat(
      vscode.Uri.file(path.join(workspacePath, ".vscode"))
    );
    const yesNo = await vscode.window.showInformationMessage(
      vscode.l10n.t(
        "A .vscode directory already exists. Do you want to overwrite it?"
      ),
      "Yes",
      "No"
    );
    if (yesNo !== "Yes") {
      return;
    }
  } catch (e) {}

  await vscode.workspace.fs.createDirectory(
    vscode.Uri.file(path.join(workspacePath, "attachments"))
  );

  // create file and open
  const file = vscode.Uri.file(
    path.join(workspacePath, ".vscode", "settings.json")
  );
  await vscode.workspace.fs.writeFile(
    file,
    Buffer.from(await getSettingsJsonString(context))
  );

  await vscode.window.showTextDocument(
    await vscode.workspace.openTextDocument(file)
  );
};

export default initializeWorkspace;
