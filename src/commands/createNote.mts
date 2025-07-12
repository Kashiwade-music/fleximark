import * as vscode from "vscode";
import * as path from "path";
import expandSnippetPlaceholders from "./utils/expandSnippetPlaceholders.mjs";

interface CategoryTree {
  [key: string]: CategoryTree | Record<string, never>;
}

interface TemplatesMap {
  [key: string]: string[];
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
          "No categories defined. Please add them in .vscode/settings.json."
        )
      );
      return "";
    }

    const selected = await vscode.window.showQuickPick(categoryKeys, {
      placeHolder: "Select a category",
      canPickMany: false,
    });

    if (!selected) {
      vscode.window.showWarningMessage(
        vscode.l10n.t("Category selection was cancelled.")
      );
      return "";
    }

    const subTree = categories[selected];
    if (typeof subTree !== "object" || subTree === null) {
      vscode.window.showErrorMessage(
        vscode.l10n.t("Invalid category structure for selected category.")
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
      vscode.l10n.t("An unexpected error occurred: {0}", String(error))
    );
    return "";
  }
}

/**
 * Prompts the user to select a template and returns its content as a SnippetString.
 */
async function promptTemplateSelection(
  templates: TemplatesMap
): Promise<vscode.SnippetString | null> {
  try {
    const templateNames = Object.keys(templates);

    if (templateNames.length === 0) {
      vscode.window.showErrorMessage(
        vscode.l10n.t(
          "No templates defined. Please add them in .vscode/settings.json."
        )
      );
      return null;
    }

    const selected = await vscode.window.showQuickPick(templateNames, {
      placeHolder: "Select a template",
      canPickMany: false,
    });

    if (!selected) {
      vscode.window.showWarningMessage(
        vscode.l10n.t("Template selection was cancelled.")
      );
      return null;
    }

    const selectedTemplate = templates[selected];
    if (!Array.isArray(selectedTemplate)) {
      vscode.window.showErrorMessage(
        vscode.l10n.t(
          "Invalid template format for '{0}'. Expected an array of strings.",
          selected
        )
      );
      return null;
    }

    const snippetContent = selectedTemplate.join("\n");
    return new vscode.SnippetString(snippetContent);
  } catch (error) {
    console.error("Error in promptTemplateSelection:", error);
    vscode.window.showErrorMessage(
      vscode.l10n.t("An unexpected error occurred: {0}", String(error))
    );
    return null;
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

/**
 * Checks whether a file already exists.
 */
async function fileExists(uri: vscode.Uri): Promise<boolean> {
  try {
    await vscode.workspace.fs.stat(uri);
    return true;
  } catch {
    return false;
  }
}

/**
 * Prompts the user to enter a valid file name.
 */
async function promptFileName(): Promise<string | undefined> {
  return vscode.window.showInputBox({
    prompt: "Enter file name",
    validateInput: (value) =>
      !value.trim() ? "File name cannot be empty." : undefined,
  });
}

/**
 * Main entry point for creating a new markdown note.
 */
async function createNote(context: vscode.ExtensionContext): Promise<void> {
  const config = vscode.workspace.getConfiguration("fleximark");
  const categories = config.get<CategoryTree>("noteCategories") ?? {};
  const templates = config.get<TemplatesMap>("noteTemplates") ?? {};
  const noteFileNamePrefix = expandSnippetPlaceholders(
    config.get<string>("noteFileNamePrefix") ?? ""
  );
  const noteFileNameSuffix = expandSnippetPlaceholders(
    config.get<string>("noteFileNameSuffix") ?? ""
  );

  const categoryPath = await promptCategoryPath(categories);
  if (!categoryPath) {
    return;
  }

  const templateSnippet = await promptTemplateSelection(templates);
  if (!templateSnippet) {
    return;
  }

  const fileName = await promptFileName();
  if (!fileName) {
    return;
  }

  const workspaceFolders = vscode.workspace.workspaceFolders;
  if (!workspaceFolders?.length) {
    vscode.window.showErrorMessage(
      vscode.l10n.t("Please open a workspace before creating a note.")
    );
    return;
  }

  const workspaceRoot = workspaceFolders[0].uri.fsPath;
  const noteDir = path.join(workspaceRoot, categoryPath);
  const noteUri = vscode.Uri.file(
    path.join(
      noteDir,
      `${noteFileNamePrefix}${fileName}${noteFileNameSuffix}.md`
    )
  );

  await ensureDirectoryExists(noteDir);

  if (await fileExists(noteUri)) {
    vscode.window.showErrorMessage(
      vscode.l10n.t("A note with that name already exists.")
    );
    return;
  }

  await vscode.workspace.fs.writeFile(noteUri, new Uint8Array());

  const document = await vscode.workspace.openTextDocument(noteUri);
  const editor = await vscode.window.showTextDocument(document);
  await editor.insertSnippet(templateSnippet);
}

export default createNote;
