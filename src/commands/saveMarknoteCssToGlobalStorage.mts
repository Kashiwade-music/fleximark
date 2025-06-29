import * as vscode from "vscode";
import * as fs from "fs/promises";
import { getDefaultMarknoteCss } from "./css/index.mjs";

/**
 * Saves the default marknote.css file to the extension's global storage directory.
 * This is typically used during initial activation or first install.
 *
 * @param context - The extension context containing the globalStorageUri.
 */
async function saveMarknoteCssToGlobalStorage(
  context: vscode.ExtensionContext
): Promise<void> {
  const cssContent = getDefaultMarknoteCss();
  const cssPath = vscode.Uri.joinPath(context.globalStorageUri, "marknote.css");

  try {
    await fs.mkdir(context.globalStorageUri.fsPath, { recursive: true });
    await fs.writeFile(cssPath.fsPath, cssContent, "utf8");
    vscode.window.showInformationMessage(
      "marknote.css has been saved to global storage."
    );
  } catch (err) {
    vscode.window.showErrorMessage(
      "Failed to save marknote.css to global storage."
    );
    console.error("Failed to save marknote.css to global storage:", err);
  }
}

export default saveMarknoteCssToGlobalStorage;
