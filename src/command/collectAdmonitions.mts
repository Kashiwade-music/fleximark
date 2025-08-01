import * as path from "path";
import * as vscode from "vscode";

import collectAdmonitionsImpl from "./collectAdmonitionsImpl/index.mjs";

interface CategoryTree {
  [key: string]: CategoryTree | Record<string, never>;
}

/**
 * Prompts the user to select a nested category path.
 * Traverses the tree recursively.
 */
async function promptCategoryPath(categories: CategoryTree): Promise<string> {
  try {
    const categoryKeys = Object.keys(categories);

    if (categoryKeys.length === 0) {
      vscode.window.showErrorMessage(
        vscode.l10n.t(
          "No categories defined. Please add them in .vscode/settings.json.",
        ),
      );
      return "";
    }

    const selected = await vscode.window.showQuickPick(categoryKeys, {
      placeHolder: vscode.l10n.t(
        "Select a  root category. Collect all Markdown admonitions/alerts belonging to the selected category.",
      ),
      canPickMany: false,
    });

    if (!selected) {
      vscode.window.showWarningMessage(
        vscode.l10n.t("Category selection was cancelled."),
      );
      return "";
    }

    const subTree = categories[selected];
    if (typeof subTree !== "object" || subTree === null) {
      vscode.window.showErrorMessage(
        vscode.l10n.t("Invalid category structure for selected category."),
      );
      return "";
    }

    const hasSubcategories = Object.keys(subTree).length > 0;

    if (hasSubcategories) {
      const subPath = await promptCategoryPath(subTree);
      if (!subPath) return selected; // Subcategory selection cancelled
      return path.join(selected, subPath);
    }

    return selected;
  } catch (error) {
    console.error("Error in promptCategoryPath:", error);
    vscode.window.showErrorMessage(
      vscode.l10n.t("An unexpected error occurred: {0}", String(error)),
    );
    return "";
  }
}

/**
 * Ensures a directory exists at the specified path.
 */
async function ensureDirectoryExists(dirPath: string): Promise<void> {
  const dirUri = vscode.Uri.file(dirPath);
  try {
    await vscode.workspace.fs.stat(dirUri);
  } catch {
    await vscode.workspace.fs.createDirectory(dirUri);
  }
}

async function promptFileName(): Promise<string | undefined> {
  return vscode.window.showInputBox({
    prompt: "Enter file name",
    validateInput: (value) =>
      !value.trim() ? "File name cannot be empty." : undefined,
    value: "[Auto Gen] Admonitions",
  });
}

async function collectAdmonitions(): Promise<void> {
  const config = vscode.workspace.getConfiguration("fleximark");
  const categories = config.get<CategoryTree>("noteCategories") ?? {};

  const categoryPath = await promptCategoryPath(categories);
  if (!categoryPath) {
    return;
  }

  const fileName = await promptFileName();
  if (!fileName) {
    return;
  }

  const workspaceFolders = vscode.workspace.workspaceFolders;
  if (!workspaceFolders?.length) {
    vscode.window.showErrorMessage(
      vscode.l10n.t("Please open a workspace before collecting admonitions."),
    );
    return;
  }

  const workspaceRoot = workspaceFolders[0].uri.fsPath;
  const noteDir = path.join(workspaceRoot, categoryPath);
  const noteUri = vscode.Uri.file(path.join(noteDir, `${fileName}.md`));

  await ensureDirectoryExists(noteDir);

  const fileContent = collectAdmonitionsImpl(noteDir);

  await vscode.workspace.fs.writeFile(
    noteUri,
    Buffer.from(fileContent, "utf-8"),
  );

  const document = await vscode.workspace.openTextDocument(noteUri);
  await vscode.window.showTextDocument(document);
}

export default collectAdmonitions;
