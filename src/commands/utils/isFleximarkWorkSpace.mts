import * as path from "path";
import * as vscode from "vscode";

const isFleximarkWorkspace = async (): Promise<boolean> => {
  const workspace = vscode.workspace.workspaceFolders;
  if (!workspace) {
    return false;
  }

  const workspacePath = workspace[0].uri.fsPath;
  const fleximarkFilePath = path.join(
    workspacePath,
    ".fleximark",
    "fleximark.json",
  );

  try {
    await vscode.workspace.fs.stat(vscode.Uri.file(fleximarkFilePath));
    return true;
  } catch {
    return false;
  }
};

export default isFleximarkWorkspace;
