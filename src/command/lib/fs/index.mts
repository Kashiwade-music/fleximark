import path from "path";
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

export function getWorkspaceFoldersOrShowError(isMessageEmittable = true) {
  const workspaceFolders = vscode.workspace.workspaceFolders;
  if (!workspaceFolders?.length) {
    if (isMessageEmittable) {
      vscode.window.showErrorMessage(vscode.l10n.t("Please open a workspace."));
    }
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

export async function isFleximarkWorkspace() {
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
}

export async function confirmOverwriteAndBackupFile(
  fileUri: vscode.Uri,
  forceOverwrite = false,
): Promise<boolean> {
  let shouldProceed = true;

  const fileExists = await isFileExists(fileUri);
  if (!fileExists) {
    return true; // No file to overwrite, proceed
  }

  try {
    if (!forceOverwrite) {
      const confirmation = await vscode.window.showInformationMessage(
        vscode.l10n.t(
          "{0} already exists. Overwrite it? A backup will be created.",
          path.basename(fileUri.fsPath),
        ),
        "Yes",
        "No",
      );

      if (confirmation !== "Yes") {
        shouldProceed = false;
      }
    }
  } catch {
    /* empty */
  }

  if (!shouldProceed) return false;

  try {
    const dir = path.dirname(fileUri.fsPath);
    const timestamp = new Date().toISOString().slice(0, 10).replace(/-/g, "");
    const backupFilename = `${path.basename(fileUri.fsPath, ".json")}_${timestamp}.json`;
    const backupPath = path.join(dir, backupFilename);
    const backupUri = vscode.Uri.file(backupPath);

    await vscode.workspace.fs.copy(fileUri, backupUri, { overwrite: true });
  } catch {
    /* empty */
  }

  return true;
}
