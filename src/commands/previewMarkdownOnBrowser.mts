import * as vscode from "vscode";
import express from "express";
import renderMarkdownToHtml from "./renderMarkdownToHtml/index.mjs";

const previewMarkdownOnBrowser = async (context: vscode.ExtensionContext) => {
  const editor = vscode.window.activeTextEditor;
  const doc = editor?.document;

  if (!editor || !doc || doc.languageId !== "markdown") {
    vscode.window.showErrorMessage(
      "Markdown ファイルがアクティブである必要があります。"
    );
    return;
  }

  try {
    const app = express();
    app.use(express.static(context.extensionPath));
    const html = await renderMarkdownToHtml(doc.getText(), context);

    return { app, html };
  } catch (err) {
    vscode.window.showErrorMessage(
      "Markdown のプレビュー生成中にエラーが発生しました。"
    );
    console.error(err);
    return;
  }
};

export default previewMarkdownOnBrowser;
