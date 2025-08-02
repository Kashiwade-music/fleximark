import * as path from "path";
import * as vscode from "vscode";

export interface CategoryTree {
  [key: string]: CategoryTree | Record<string, never>;
}

export function getCategoryTree() {
  const config = vscode.workspace.getConfiguration("fleximark");
  const categories = config.get<CategoryTree>("noteCategories") ?? {};

  return categories;
}

/**
 * Prompts the user to select a nested category path.
 * Traverses the tree recursively.
 */
export async function promptCategoryPath(
  quickPickPlaceHolder: string,
  currentSubTree?: CategoryTree,
): Promise<string> {
  try {
    const categoryTree = currentSubTree ?? getCategoryTree();
    const categoryKeys = Object.keys(categoryTree);

    if (categoryKeys.length === 0) {
      vscode.window.showErrorMessage(
        vscode.l10n.t(
          "No categories defined. Please add them in .vscode/settings.json.",
        ),
      );
      return "";
    }

    const selected = await vscode.window.showQuickPick(categoryKeys, {
      placeHolder: quickPickPlaceHolder,
      canPickMany: false,
    });

    if (!selected) {
      vscode.window.showWarningMessage(
        vscode.l10n.t("Category selection was cancelled."),
      );
      return "";
    }

    const subTree = categoryTree[selected];
    if (typeof subTree !== "object" || subTree === null) {
      vscode.window.showErrorMessage(
        vscode.l10n.t("Invalid category structure for selected category."),
      );
      return "";
    }

    const hasSubcategories = Object.keys(subTree).length > 0;

    if (hasSubcategories) {
      const subPath = await promptCategoryPath(quickPickPlaceHolder, subTree);
      if (!subPath) return selected; // Subcategory selection cancelled
      return path.join(selected, subPath);
    }

    return selected;
  } catch (error) {
    console.error("Error in getCategoryPath:", error);
    vscode.window.showErrorMessage(
      vscode.l10n.t("An unexpected error occurred: {0}", String(error)),
    );
    return "";
  }
}
