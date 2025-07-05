import * as vscode from "vscode";
import express from "express";
import renderMarkdownToHtml from "./renderMarkdownToHtml/index.mjs";

const previewMarkdownOnBrowser = async (context: vscode.ExtensionContext) => {
  const editor = vscode.window.activeTextEditor;
  const doc = editor?.document;

  if (!editor || !doc || doc.languageId !== "markdown") {
    vscode.window.showErrorMessage(
      vscode.l10n.t("The Markdown file must be active.")
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
      vscode.l10n.t("An error occurred while preparing the Markdown preview.")
    );
    console.error(err);
    return;
  }
};

export default previewMarkdownOnBrowser;
