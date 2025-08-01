import * as vscode from "vscode";

import convertMdToHtml from "./renderMarkdownToHtml/index.mjs";

const previewMarkdownOnVscode = async (context: vscode.ExtensionContext) => {
  const editorPanel = vscode.window.activeTextEditor;
  const doc = editorPanel?.document;

  if (!editorPanel || !doc || doc.languageId !== "markdown") {
    vscode.window.showErrorMessage(
      vscode.l10n.t("The Markdown file must be active."),
    );
    return;
  }

  try {
    const webviewPanel = vscode.window.createWebviewPanel(
      "markdownPreview",
      "Markdown Preview",
      vscode.ViewColumn.Beside,
      { enableScripts: true },
    );

    const res = await convertMdToHtml(
      doc.getText(),
      doc.uri.fsPath,
      context,
      webviewPanel.webview,
    );

    webviewPanel.webview.html = res.html;
    return { webviewPanel, editorPanel, ...res };
  } catch {
    vscode.window.showErrorMessage(
      vscode.l10n.t("An error occurred while preparing the Markdown preview."),
    );
    return;
  }
};

export default previewMarkdownOnVscode;
