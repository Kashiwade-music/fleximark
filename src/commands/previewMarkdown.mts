import * as vscode from "vscode";
import renderMarkdownToHtml from "./renderMarkdownToHtml/index.mjs";

const previewMarkdown = async (context: vscode.ExtensionContext) => {
  const editor = vscode.window.activeTextEditor;
  const doc = editor?.document;

  if (!editor || !doc || doc.languageId !== "markdown") {
    vscode.window.showErrorMessage(
      vscode.l10n.t("The Markdown file must be active.")
    );
    return;
  }

  try {
    const panel = vscode.window.createWebviewPanel(
      "markdownPreview",
      "Markdown Preview",
      vscode.ViewColumn.Beside,
      { enableScripts: true }
    );

    const res = await renderMarkdownToHtml(
      doc.getText(),
      context,
      panel.webview
    );

    panel.webview.html = res.html;
    return panel;
  } catch (err) {
    vscode.window.showErrorMessage(
      vscode.l10n.t("An error occurred while preparing the Markdown preview.")
    );
    return;
  }
};

export default previewMarkdown;
