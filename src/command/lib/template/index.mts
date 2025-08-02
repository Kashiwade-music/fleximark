import * as vscode from "vscode";

export type TemplateMap = Record<string, string[]>;

/**
 * Prompts the user to select a template and returns its content as a SnippetString.
 */
export async function promptTemplate(): Promise<vscode.SnippetString | null> {
  try {
    const config = vscode.workspace.getConfiguration("fleximark");
    const templates = config.get<TemplateMap>("noteTemplates") ?? {};

    const templateNames = Object.keys(templates);

    if (templateNames.length === 0) {
      vscode.window.showErrorMessage(
        vscode.l10n.t(
          "No templates defined. Please add them in .vscode/settings.json.",
        ),
      );
      return null;
    }

    const selected = await vscode.window.showQuickPick(templateNames, {
      placeHolder: "Select a template",
      canPickMany: false,
    });

    if (!selected) {
      vscode.window.showWarningMessage(
        vscode.l10n.t("Template selection was cancelled."),
      );
      return null;
    }

    const selectedTemplate = templates[selected];
    if (!Array.isArray(selectedTemplate)) {
      vscode.window.showErrorMessage(
        vscode.l10n.t(
          "Invalid template format for '{0}'. Expected an array of strings.",
          selected,
        ),
      );
      return null;
    }

    const snippetContent = selectedTemplate.join("\n");
    return new vscode.SnippetString(snippetContent);
  } catch (error) {
    console.error("Error in promptTemplateSelection:", error);
    vscode.window.showErrorMessage(
      vscode.l10n.t("An unexpected error occurred: {0}", String(error)),
    );
    return null;
  }
}
