// The module 'vscode' contains the VS Code extensibility API
// Import the module and reference it with the alias vscode in your code below
import * as vscode from "vscode";
import previewMarkdown from "./commands/previewMarkdown.mjs";
import exportHtml from "./commands/exportHtml.mjs";
import createWorkspaceMarknoteCss from "./commands/createWorkspaceMarknoteCss.mjs";
import saveMarknoteCssToGlobalStorage from "./commands/saveMarknoteCssToGlobalStorage.mjs";
import renderMarkdownToHtml from "./commands/renderMarkdownToHtml/index.mjs";
import { isGlobalMarknoteCssExists } from "./commands/css/index.mjs";
import previewMarkdownOnBrowser from "./commands/previewMarkdownOnBrowser.mjs";
import express, { Express, Request, Response } from "express";

// This method is called when your extension is activated
// Your extension is activated the very first time the command is executed
export function activate(context: vscode.ExtensionContext) {
  // This line of code will only be executed once when your extension is activated
  console.log('Congratulations, your extension "marknote" is now active!');

  // ==========================
  //
  // Initial setup
  //
  // ==========================

  if (!isGlobalMarknoteCssExists(context)) {
    saveMarknoteCssToGlobalStorage(context);
  }

  // if in Dev, comment out the next line
  saveMarknoteCssToGlobalStorage(context);

  // ==========================
  //
  // Global variables
  //
  // ==========================

  // VSCode Webview Panel for Markdown Preview
  let panel: vscode.WebviewPanel | undefined;

  // Express app for Markdown Preview in Browser
  let app: Express | undefined;
  let appHtml = "";

  const updatePreview = async (doc: vscode.TextDocument) => {
    if (doc.languageId !== "markdown") return;

    const html = await renderMarkdownToHtml(
      doc.getText(),
      context,
      panel?.webview
    );
    if (panel) panel.webview.html = html;
    if (app) appHtml = html;
  };

  // ==========================
  //
  // Utility functions
  //
  // ==========================

  const startBrowserPreviewServer = (app: Express, html: string) => {
    const port =
      vscode.workspace
        .getConfiguration("marknote")
        .get<number>("browserPreviewPort") || 3000;

    app.get("/", (req: Request, res: Response) => {
      res.send(appHtml);
    });
    app.listen(port, () => {
      vscode.window.showInformationMessage(
        vscode.l10n.t(
          "The Markdown preview has opened at http://localhost:{port}.",
          { port }
        )
      );
      vscode.env.openExternal(vscode.Uri.parse(`http://localhost:${port}`));
    });
  };

  // ==========================
  //
  // Register commands
  //
  // ==========================

  // The command has been defined in the package.json file
  // Now provide the implementation of the command with registerCommand
  // The commandId parameter must match the command field in package.json

  context.subscriptions.push(
    vscode.commands.registerCommand("marknote.previewMarkdown", async () => {
      const mode =
        vscode.workspace
          .getConfiguration("marknote")
          .get<string>("defaultPreviewMode") || "vscode";

      if (mode === "vscode") {
        panel = await previewMarkdown(context);
        panel?.onDidDispose(() => (panel = undefined));
      } else if (mode === "browser") {
        const result = await previewMarkdownOnBrowser(context);
        if (result) {
          ({ app, html: appHtml } = result);
          startBrowserPreviewServer(app, appHtml);
        }
      }
    })
  );

  context.subscriptions.push(
    vscode.commands.registerCommand(
      "marknote.previewMarkdownOnVscode",
      async () => {
        panel = await previewMarkdown(context);
        panel?.onDidDispose(() => {
          panel = undefined;
        });
      }
    )
  );

  context.subscriptions.push(
    vscode.commands.registerCommand(
      "marknote.previewMarkdownOnBrowser",
      async () => {
        const result = await previewMarkdownOnBrowser(context);
        if (result) {
          ({ app, html: appHtml } = result);
          startBrowserPreviewServer(app, appHtml);
        }
      }
    )
  );

  context.subscriptions.push(
    vscode.commands.registerCommand("marknote.exportHtml", async () => {
      await exportHtml(context);
    })
  );

  context.subscriptions.push(
    vscode.commands.registerCommand(
      "marknote.createWorkspaceMarknoteCss",
      async () => {
        await createWorkspaceMarknoteCss(context);
      }
    )
  );

  context.subscriptions.push(
    vscode.commands.registerCommand(
      "marknote.resetGlobalMarknoteCss",
      async () => {
        await saveMarknoteCssToGlobalStorage(context);
      }
    )
  );

  context.subscriptions.push(
    vscode.window.onDidChangeActiveTextEditor(
      (editor) => editor && updatePreview(editor.document)
    ),
    vscode.workspace.onDidChangeTextDocument((event) =>
      updatePreview(event.document)
    )
  );
}

// This method is called when your extension is deactivated
export function deactivate() {}
