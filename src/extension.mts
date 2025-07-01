// The module 'vscode' contains the VS Code extensibility API
// Import the module and reference it with the alias vscode in your code below
import * as vscode from "vscode";
import previewMarkdown from "./commands/previewMarkdown.mjs";
import exportHtml from "./commands/exportHtml.mjs";
import createWorkspaceMarknoteCss from "./commands/createWorkspaceMarknoteCss.mjs";
import saveMarknoteCssToGlobalStorage from "./commands/saveMarknoteCssToGlobalStorage.mjs";
import renderMarkdownToHtml from "./commands/renderMarkdownToHtml/index.mjs";
import { isGlobalMarknoteCssExists } from "./commands/css/index.mjs";
import * as playwright from "playwright";

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

  // ==========================
  //
  // Global variables
  //
  // ==========================

  let panel: vscode.WebviewPanel | undefined;

  const updatePreview = async (doc: vscode.TextDocument) => {
    if (panel && doc.languageId === "markdown") {
      const html = await renderMarkdownToHtml(doc.getText(), context);
      panel.webview.html = html;
    }
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
    vscode.commands.registerCommand(
      "marknote.previewMarkdownAsHtml",
      async () => {
        panel = await previewMarkdown(context);
      }
    )
  );

  // hot-reload the preview when the active text editor changes
  context.subscriptions.push(
    vscode.window.onDidChangeActiveTextEditor((editor) => {
      if (editor && editor.document.languageId === "markdown") {
        updatePreview(editor.document);
      }
    })
  );

  // hot-reload the preview when the text document is changed
  context.subscriptions.push(
    vscode.workspace.onDidChangeTextDocument((event) => {
      if (panel && event.document.languageId === "markdown") {
        updatePreview(event.document);
      }
    })
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
}

// This method is called when your extension is deactivated
export function deactivate() {}
