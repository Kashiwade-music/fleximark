import * as fs from "fs/promises";
import * as vscode from "vscode";

import * as fLibCss from "../../lib/css/index.mjs";

async function resetGlobalFleximarkCss(
  context: vscode.ExtensionContext,
): Promise<void> {
  const cssContent = fLibCss.getDefaultFleximarkCss();
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

export default resetGlobalFleximarkCss;
