import * as vscode from "vscode";
import express from "express";
import renderMarkdownToHtml from "./renderMarkdownToHtml/index.mjs";

const previewMarkdownOnBrowser = async (context: vscode.ExtensionContext) => {
  const editorPanel = vscode.window.activeTextEditor;
  const doc = editorPanel?.document;

  if (!editorPanel || !doc || doc.languageId !== "markdown") {
    vscode.window.showErrorMessage(
      vscode.l10n.t("The Markdown file must be active.")
    );
    return;
  }

  try {
    const app = express();
    app.use(express.static(context.extensionPath));
    const res = await renderMarkdownToHtml(doc.getText(), context);

    return { app, editorPanel, ...res };
  } catch (err) {
    vscode.window.showErrorMessage(
      vscode.l10n.t("An error occurred while preparing the Markdown preview.")
    );
    console.error(err);
    return;
  }
};

export default previewMarkdownOnBrowser;
