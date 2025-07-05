import * as vscode from "vscode";
import * as fs from "fs/promises";
import marknotecss from "./marknote.css";

/**
 * Returns the default content for the marknote.css file.
 * This CSS can be used as a base styling.
 */
export function getDefaultMarknoteCss(): string {
  return marknotecss;
}

/**
 * Checks whether the global marknote.css file exists in the extension's global storage.
 *
 * @param context - The extension context containing the globalStorageUri.
 * @returns A promise that resolves to true if the file exists, or false otherwise.
 */
export async function isGlobalMarknoteCssExists(
  context: vscode.ExtensionContext
): Promise<boolean> {
  const cssPath = vscode.Uri.joinPath(context.globalStorageUri, "marknote.css");
  try {
    await fs.access(cssPath.fsPath);
    return true; // ファイルが存在する場合
  } catch (err) {
    return false; // ファイルが存在しない場合
  }
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
      vscode.l10n.t(
        "marknote.css not found in global storage, returning default content."
      )
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
