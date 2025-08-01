import express from "express";
import path from "path";
import * as vscode from "vscode";

import convertMdToHtml from "./renderMarkdownToHtml/index.mjs";

const previewMarkdownOnBrowser = async (context: vscode.ExtensionContext) => {
  const editorPanel = vscode.window.activeTextEditor;
  const doc = editorPanel?.document;

  if (!editorPanel || !doc || doc.languageId !== "markdown") {
    vscode.window.showErrorMessage(
      vscode.l10n.t("The Markdown file must be active."),
    );
    return;
  }

  const workspaceFolder = vscode.workspace.getWorkspaceFolder(doc.uri);
  if (!workspaceFolder) {
    vscode.window.showErrorMessage(
      vscode.l10n.t("The Markdown file must be in a workspace folder."),
    );
    return;
  }

  try {
    const app = express();
    app.use(express.static(context.extensionPath));
    app.use(express.static(workspaceFolder.uri.fsPath));
    app.use(express.static(path.dirname(doc.uri.fsPath)));

    const res = await convertMdToHtml({
      convertType: "browser",
      markdown: doc.getText(),
      context: context,
      isNeedDataLineNumber: true,
    });

    return { app, editorPanel, ...res };
  } catch (err) {
    vscode.window.showErrorMessage(
      vscode.l10n.t("An error occurred while preparing the Markdown preview."),
    );
    console.error(err);
    return;
  }
};

export default previewMarkdownOnBrowser;
