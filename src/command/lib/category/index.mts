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

const FINALIZE_CATEGORY = "final_OGxcnkhJMR";

/**
 * Prompts the user to select a nested category path.
 * Traverses the tree recursively.
 */
export async function promptCategoryPath(
  quickPickPlaceHolder: string,
  currentSubTree?: CategoryTree,
  currentPath: string[] = [],
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

    const selectedQuickPickItem = await vscode.window.showQuickPick(
      categoryKeysToQuickPickItems(categoryKeys, currentPath),
      {
        placeHolder: quickPickPlaceHolder,
        canPickMany: false,
        ignoreFocusOut: true,
      },
    );
    if (!selectedQuickPickItem) {
      vscode.window.showWarningMessage(
        vscode.l10n.t("Category selection was cancelled."),
      );
      return "";
    }

    const selected =
      selectedQuickPickItem.identifier ?? selectedQuickPickItem.label;

    if (selected === FINALIZE_CATEGORY) {
      return "/";
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
      const subPath = await promptCategoryPath(quickPickPlaceHolder, subTree, [
        ...currentPath,
        selected,
      ]);
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

interface CategoryQuickPickItem extends vscode.QuickPickItem {
  identifier?: string;
}

function categoryKeysToQuickPickItems(
  categoryKeys: string[],
  currentPath: string[] = [],
): CategoryQuickPickItem[] {
  const ret: CategoryQuickPickItem[] = categoryKeys.map((key) => ({
    label: key,
  }));

  ret.push({
    label: "Dialog",
    kind: vscode.QuickPickItemKind.Separator,
  });

  let currentPathLabel = currentPath.join("/");
  if (currentPathLabel === "") {
    currentPathLabel = "./";
  }

  ret.push({
    label: vscode.l10n.t("Confirm the previously selected category as final"),
    identifier: FINALIZE_CATEGORY,
    detail: vscode.l10n.t("Current category: {0}", currentPathLabel),
  });

  return ret;
}
