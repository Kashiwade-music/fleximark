import * as vscode from "vscode";
import * as fs from "fs/promises";
import { getDefaultFleximarkCss } from "./css/index.mjs";

/**
 * Saves the default fleximark.css file to the extension's global storage directory.
 * This is typically used during initial activation or first install.
 *
 * @param context - The extension context containing the globalStorageUri.
 */
async function saveFleximarkCssToGlobalStorage(
  context: vscode.ExtensionContext,
): Promise<void> {
  const cssContent = getDefaultFleximarkCss();
  const cssPath = vscode.Uri.joinPath(
    context.globalStorageUri,
    "fleximark.css",
  );

  try {
    await fs.mkdir(context.globalStorageUri.fsPath, { recursive: true });
    await fs.writeFile(cssPath.fsPath, cssContent, "utf8");
    vscode.window.showInformationMessage(
      vscode.l10n.t("fleximark.css has been saved to global storage."),
    );
  } catch {
    vscode.window.showErrorMessage(
      vscode.l10n.t("Failed to save fleximark.css to global storage."),
    );
  }
}

export default saveFleximarkCssToGlobalStorage;
