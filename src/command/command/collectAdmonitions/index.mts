import * as path from "path";
import * as vscode from "vscode";

import * as fLibCategory from "../../lib/category/index.mjs";
import * as fLibFs from "../../lib/fs/index.mjs";
import process from "./process.mjs";

async function collectAdmonitions(): Promise<void> {
  const categoryPath = await fLibCategory.promptCategoryPath(
    vscode.l10n.t("Select a root category to collect Markdown admonitions."),
  );
  if (!categoryPath) {
    return;
  }

  const fileName = await fLibFs.promptFileName("[Auto Gen] Admonitions");
  if (!fileName) {
    return;
  }

  const workspaceFolders = fLibFs.getWorkspaceFoldersOrShowError();
  if (!workspaceFolders) {
    return;
  }

  const workspaceRoot = workspaceFolders[0].uri.fsPath;
  const noteDir = path.join(workspaceRoot, categoryPath);
  const noteUri = vscode.Uri.file(path.join(noteDir, `${fileName}.md`));

  await fLibFs.ensureDirectoryExists(noteDir);

  const fileContent = process(noteDir);

  await vscode.workspace.fs.writeFile(
    noteUri,
    Buffer.from(fileContent, "utf-8"),
  );

  const document = await vscode.workspace.openTextDocument(noteUri);
  await vscode.window.showTextDocument(document);
}

export default collectAdmonitions;
