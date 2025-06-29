import * as vscode from "vscode";
import * as fs from "fs/promises";

/**
 * Returns the default content for the marknote.css file.
 * This CSS can be used as a base styling.
 */
export function getDefaultMarknoteCss(): string {
  return `/* Default marknote.css */
/* Customize your styles here */`;
}

/**
 * Reads the marknote.css file from the extension's global storage.
 * If the file does not exist, returns the default CSS content and attempts to create it.
 *
 * @param context - The extension context containing the globalStorageUri.
 * @returns The content of the marknote.css file as a string.
 */
export async function readGlobalMarknoteCss(
  context: vscode.ExtensionContext
): Promise<string> {
  const cssPath = vscode.Uri.joinPath(context.globalStorageUri, "marknote.css");
  try {
    const content = await fs.readFile(cssPath.fsPath, "utf8");
    return content;
  } catch (err) {
    // ファイルが存在しない場合はデフォルトの内容を返す
    vscode.window.showWarningMessage(
      "marknote.css not found in global storage, returning default content."
    );
    console.warn(
      "marknote.css not found in global storage, returning default content."
    );
    return getDefaultMarknoteCss();
  }
}

/**
 * Reads the marknote.css file from the current workspace's .marknote directory.
 * If the file does not exist or cannot be read, returns an empty string.
 *
 * @returns The content of the workspace marknote.css file or an empty string.
 */
export async function readWorkspaceMarknoteCss(): Promise<string> {
  const workspaceFolders = vscode.workspace.workspaceFolders;
  if (!workspaceFolders) return "";

  const workspaceUri = workspaceFolders[0].uri;
  const cssPath = vscode.Uri.joinPath(
    workspaceUri,
    ".marknote",
    "marknote.css"
  );

  try {
    const content = await fs.readFile(cssPath.fsPath, "utf8");
    return content;
  } catch {
    // ファイルが存在しない or 読み込めない場合
    return "";
  }
}
