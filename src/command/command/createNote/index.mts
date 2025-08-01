import * as path from "path";
import * as vscode from "vscode";

import * as fLibCategory from "../../lib/category/index.mjs";
import * as fLibFs from "../../lib/fs/index.mjs";
import * as fLibSnippet from "../../lib/snippet/index.mjs";
import * as FLibTemplate from "../../lib/template/index.mjs";

/**
 * Main entry point for creating a new markdown note.
 */
async function createNote(): Promise<void> {
  const config = vscode.workspace.getConfiguration("fleximark");
  const noteFileNamePrefix = fLibSnippet.expandPlaceholders(
    config.get<string>("noteFileNamePrefix") ?? "",
  );
  const noteFileNameSuffix = fLibSnippet.expandPlaceholders(
    config.get<string>("noteFileNameSuffix") ?? "",
  );

  const categoryPath = await fLibCategory.promptCategoryPath(
    vscode.l10n.t("Select a category"),
  );
  if (!categoryPath) {
    return;
  }

  const templateSnippet = await FLibTemplate.promptTemplate();
  if (!templateSnippet) {
    return;
  }

  const fileName = await fLibFs.promptFileName();
  if (!fileName) {
    return;
  }

  const workspaceFolders = fLibFs.getWorkspaceFoldersOrShowError();
  if (!workspaceFolders) {
    return;
  }

  const workspaceRoot = workspaceFolders[0].uri.fsPath;
  const noteDir = path.join(workspaceRoot, categoryPath);
  const noteUri = vscode.Uri.file(
    path.join(
      noteDir,
      `${noteFileNamePrefix}${fileName}${noteFileNameSuffix}.md`,
    ),
  );

  await fLibFs.ensureDirectoryExists(noteDir);

  if (await fLibFs.isFileExists(noteUri)) {
    vscode.window.showErrorMessage(
      vscode.l10n.t("A note with that name already exists."),
    );
    return;
  }

  await vscode.workspace.fs.writeFile(noteUri, new Uint8Array());

  const document = await vscode.workspace.openTextDocument(noteUri);
  const editor = await vscode.window.showTextDocument(document);
  await editor.insertSnippet(templateSnippet);
}

export default createNote;
