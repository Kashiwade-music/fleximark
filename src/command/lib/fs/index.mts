import * as vscode from "vscode";

/**
 * Ensures a directory exists at the specified path.
 */
export async function ensureDirectoryExists(dirPath: string): Promise<void> {
  const dirUri = vscode.Uri.file(dirPath);
  try {
    await vscode.workspace.fs.stat(dirUri);
  } catch {
    await vscode.workspace.fs.createDirectory(dirUri);
  }
}

export async function promptFileName(
  defaultValue?: string,
): Promise<string | undefined> {
  return await vscode.window.showInputBox({
    prompt: vscode.l10n.t("Enter file name"),
    validateInput: (value) =>
      !value.trim() ? vscode.l10n.t("File name cannot be empty.") : undefined,
    value: defaultValue,
  });
}

export function getWorkspaceFoldersOrShowError() {
  const workspaceFolders = vscode.workspace.workspaceFolders;
  if (!workspaceFolders?.length) {
    vscode.window.showErrorMessage(vscode.l10n.t("Please open a workspace."));
    return;
  }
  return workspaceFolders;
}

export async function isFileExists(uri: vscode.Uri): Promise<boolean> {
  try {
    await vscode.workspace.fs.stat(uri);
    return true;
  } catch {
    return false;
  }
}
