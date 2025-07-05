import * as vscode from "vscode";
import * as fs from "fs/promises";

/**
 * Creates a `.marknote` directory in the workspace and places a marknote.css file inside.
 * The file contains comments pointing to the global default styles as a reference.
 *
 * @param context - The extension context used to locate the global marknote.css path.
 */
async function createWorkspaceMarknoteCss(
  context: vscode.ExtensionContext
): Promise<void> {
  const workspaceFolders = vscode.workspace.workspaceFolders;
  if (!workspaceFolders) return;

  const workspaceUri = workspaceFolders[0].uri;
  const marknoteDir = vscode.Uri.joinPath(workspaceUri, ".marknote");
  const workspaceCssPath = vscode.Uri.joinPath(marknoteDir, "marknote.css");
  const globalCssPath = vscode.Uri.joinPath(
    context.globalStorageUri,
    "marknote.css"
  );

  const commentContent = `/* 
 * This file is used to override default styles.
 * Default styles are located at:
 * ${globalCssPath.fsPath}
 * 
 * Please edit this file to customize marknote appearance.
 */`;

  try {
    await fs.mkdir(marknoteDir.fsPath, { recursive: true });
    await fs.writeFile(workspaceCssPath.fsPath, commentContent, "utf8");
    vscode.window.showInformationMessage(
      vscode.l10n.t(
        "Created .marknote directory and marknote.css file in the workspace."
      )
    );
  } catch (err) {
    vscode.window.showErrorMessage(
      vscode.l10n.t(
        "Failed to create .marknote directory or marknote.css file."
      )
    );
  }
}

export default createWorkspaceMarknoteCss;
