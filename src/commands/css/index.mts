import * as fs from "fs/promises";
import * as vscode from "vscode";

import fleximarkcss from "./fleximark.css";

/**
 * Returns the default content for the fleximark.css file.
 * This CSS can be used as a base styling.
 */
export function getDefaultFleximarkCss(): string {
  return fleximarkcss;
}

/**
 * Checks whether the global fleximark.css file exists in the extension's global storage.
 *
 * @param context - The extension context containing the globalStorageUri.
 * @returns A promise that resolves to true if the file exists, or false otherwise.
 */
export async function isGlobalFleximarkCssExists(
  context: vscode.ExtensionContext,
): Promise<boolean> {
  const cssPath = vscode.Uri.joinPath(
    context.globalStorageUri,
    "fleximark.css",
  );
  try {
    await fs.access(cssPath.fsPath);
    return true; // ファイルが存在する場合
  } catch {
    return false; // ファイルが存在しない場合
  }
}

/**
 * Reads the fleximark.css file from the extension's global storage.
 * If the file does not exist, returns the default CSS content and attempts to create it.
 *
 * @param context - The extension context containing the globalStorageUri.
 * @returns The content of the fleximark.css file as a string.
 */
export async function readGlobalFleximarkCss(
  context: vscode.ExtensionContext,
): Promise<string> {
  const cssPath = vscode.Uri.joinPath(
    context.globalStorageUri,
    "fleximark.css",
  );
  try {
    const content = await fs.readFile(cssPath.fsPath, "utf8");
    return content;
  } catch {
    vscode.window.showWarningMessage(
      vscode.l10n.t(
        "fleximark.css not found in global storage, returning default content.",
      ),
    );

    return getDefaultFleximarkCss();
  }
}

/**
 * Reads the fleximark.css file from the current workspace's .fleximark directory.
 * If the file does not exist or cannot be read, returns an empty string.
 *
 * @returns The content of the workspace fleximark.css file or an empty string.
 */
export async function readWorkspaceFleximarkCss(): Promise<string> {
  const workspaceFolders = vscode.workspace.workspaceFolders;
  if (!workspaceFolders) return "";

  const workspaceUri = workspaceFolders[0].uri;
  const cssPath = vscode.Uri.joinPath(
    workspaceUri,
    ".fleximark",
    "fleximark.css",
  );

  try {
    const content = await fs.readFile(cssPath.fsPath, "utf8");
    return content;
  } catch {
    // ファイルが存在しない or 読み込めない場合
    return "";
  }
}
