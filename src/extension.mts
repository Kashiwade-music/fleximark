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
import WebSocket, { WebSocketServer } from "ws";
import { findDiff } from "./commands/utils/diffHTML.mjs";
import { Root } from "hast";

// This method is called when your extension is activated
// Your extension is activated the very first time the command is executed
export function activate(context: vscode.ExtensionContext) {
  console.log("Marknote extension activated.");

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
  let appHtml: string | undefined;
  let appHast: Root | undefined;
  let wss: WebSocketServer | undefined;
  let clients: Set<WebSocket> = new Set();

  const updateVscodePreview = async (doc: vscode.TextDocument) => {
    if (doc.languageId !== "markdown") return;

    if (panel) {
      const res = await renderMarkdownToHtml(
        doc.getText(),
        context,
        panel?.webview
      );

      panel.webview.html = res.html;
    }
  };

  const updateBrowserPreview = async (
    doc: vscode.TextDocument,
    shouldAllReload: boolean = false
  ) => {
    if (doc.languageId !== "markdown") return;

    if (app) {
      const res = await renderMarkdownToHtml(doc.getText(), context);

      if (shouldAllReload || !appHast || !appHtml) {
        // If shouldAllReload is true, replace the entire HTML
        appHtml = res.html;
        appHast = res.hast;
        if (wss) {
          for (const client of clients) {
            if (client.readyState === WebSocket.OPEN) {
              client.send(
                JSON.stringify({
                  type: "reload",
                })
              );
            }
          }
        }
      } else {
        // Otherwise, find the first diff and update only that part
        // This is useful for live updates without reloading the entire page

        // @ts-ignore
        const htmlEditScript = findDiff(appHast, res.hast);
        appHtml = res.html;
        appHast = res.hast;

        if (wss && htmlEditScript.length > 0) {
          // Broadcast the new HTML to all connected WebSocket clients
          for (const client of clients) {
            if (client.readyState === WebSocket.OPEN) {
              console.log(
                "Sending update to client:",
                JSON.stringify({ type: "edit", htmlEditScript })
              );
              client.send(JSON.stringify({ type: "edit", htmlEditScript }));
            }
          }
        }
      }
    }
  };

  // ==========================
  //
  // Utility functions
  //
  // ==========================

  const startBrowserPreviewServer = (app: Express) => {
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

    wss = new WebSocketServer({ port: port + 1 });
    wss.on("connection", (ws) => {
      clients.add(ws);
      ws.on("close", () => {
        clients.delete(ws);
      });
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
        if (!panel) {
          panel = await previewMarkdown(context);
          panel?.onDidDispose(() => (panel = undefined));
        }
      } else if (mode === "browser") {
        if (!app) {
          const result = await previewMarkdownOnBrowser(context);
          if (result) {
            ({ app, html: appHtml, hast: appHast } = result);
            startBrowserPreviewServer(app);
          }
        } else {
          const port =
            vscode.workspace
              .getConfiguration("marknote")
              .get<number>("browserPreviewPort") || 3000;
          vscode.env.openExternal(vscode.Uri.parse(`http://localhost:${port}`));
        }
      }
    })
  );

  context.subscriptions.push(
    vscode.commands.registerCommand(
      "marknote.previewMarkdownOnVscode",
      async () => {
        if (!panel) {
          panel = await previewMarkdown(context);
          panel?.onDidDispose(() => {
            panel = undefined;
          });
        }
      }
    )
  );

  context.subscriptions.push(
    vscode.commands.registerCommand(
      "marknote.previewMarkdownOnBrowser",
      async () => {
        if (!app) {
          const result = await previewMarkdownOnBrowser(context);
          if (result) {
            ({ app, html: appHtml, hast: appHast } = result);
            startBrowserPreviewServer(app);
          }
        } else {
          const port =
            vscode.workspace
              .getConfiguration("marknote")
              .get<number>("browserPreviewPort") || 3000;
          vscode.env.openExternal(vscode.Uri.parse(`http://localhost:${port}`));
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
    vscode.window.onDidChangeActiveTextEditor((editor) => {
      if (editor && editor.document.languageId === "markdown") {
        updateVscodePreview(editor.document);
        updateBrowserPreview(editor.document, true);
      }
    }),
    vscode.workspace.onDidChangeTextDocument((event) => {
      updateVscodePreview(event.document);
      updateBrowserPreview(event.document);
    })
  );
}

// This method is called when your extension is deactivated
export function deactivate() {}
