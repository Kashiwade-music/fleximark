import * as vscode from "vscode";
import renderMarkdownToHtml from "./renderMarkdownToHtml/index.mjs";

const previewMarkdown = async (context: vscode.ExtensionContext) => {
  const editor = vscode.window.activeTextEditor;
  const doc = editor?.document;

  if (!editor || !doc || doc.languageId !== "markdown") {
    vscode.window.showErrorMessage(
      "Markdown ファイルがアクティブである必要があります。"
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

    const html = await renderMarkdownToHtml(
      doc.getText(),
      context,
      panel.webview
    );

    panel.webview.html = html;
    return panel;
  } catch (err) {
    vscode.window.showErrorMessage(
      "Markdown のプレビュー生成中にエラーが発生しました。"
    );
    console.error(err);
    return;
  }
};

export default previewMarkdown;
