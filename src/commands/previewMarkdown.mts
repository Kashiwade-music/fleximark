import * as vscode from "vscode";
import renderMarkdownToHtml from "./remark/renderMarkdownToHtml.mjs";

const previewMarkdown = async () => {
  const editor = vscode.window.activeTextEditor;
  const doc = editor?.document;

  if (!editor || !doc || doc.languageId !== "markdown") {
    vscode.window.showErrorMessage(
      "Markdown ファイルがアクティブである必要があります。"
    );
    return;
  }

  try {
    const html = await renderMarkdownToHtml(doc.getText());
    showPreviewPanel(html);
  } catch (err) {
    vscode.window.showErrorMessage(
      "Markdown のプレビュー生成中にエラーが発生しました。"
    );
    console.error(err);
  }
};

const showPreviewPanel = (htmlContent: string) => {
  const panel = vscode.window.createWebviewPanel(
    "markdownPreview",
    "Markdown Preview",
    vscode.ViewColumn.Beside,
    { enableScripts: true }
  );
};

export default previewMarkdown;
