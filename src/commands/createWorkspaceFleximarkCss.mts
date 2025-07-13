import * as fs from "fs/promises";
import * as vscode from "vscode";

/**
 * Creates a `.fleximark` directory in the workspace and places a fleximark.css file inside.
 * The file contains comments pointing to the global default styles as a reference.
 *
 * @param context - The extension context used to locate the global fleximark.css path.
 */
async function createWorkspaceFleximarkCss(
  context: vscode.ExtensionContext,
): Promise<void> {
  const workspaceFolders = vscode.workspace.workspaceFolders;
  if (!workspaceFolders) return;

  const workspaceUri = workspaceFolders[0].uri;
  const fleximarkDir = vscode.Uri.joinPath(workspaceUri, ".fleximark");
  const workspaceCssPath = vscode.Uri.joinPath(fleximarkDir, "fleximark.css");
  const globalCssPath = vscode.Uri.joinPath(
    context.globalStorageUri,
    "fleximark.css",
  );

  const commentContent = `/* 
 * This file is used to override default styles.
 * Default styles are located at:
 * ${globalCssPath.fsPath}
 * 
 * Please edit this file to customize fleximark appearance.
 */`;

  try {
    await fs.mkdir(fleximarkDir.fsPath, { recursive: true });
    await fs.writeFile(workspaceCssPath.fsPath, commentContent, "utf8");
    vscode.window.showInformationMessage(
      vscode.l10n.t(
        "Created .fleximark directory and fleximark.css file in the workspace.",
      ),
    );
  } catch {
    vscode.window.showErrorMessage(
      vscode.l10n.t(
        "Failed to create .fleximark directory or fleximark.css file.",
      ),
    );
  }
}

export default createWorkspaceFleximarkCss;
