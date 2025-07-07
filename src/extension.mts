import * as vscode from "vscode";
import previewMarkdownOnVscode from "./commands/previewMarkdownOnVscode.mjs";
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

// ==========================
//
// Global State
//
// ==========================

interface ScrollState {
  enabled: boolean;
  timer: ReturnType<typeof setTimeout> | null;
}

const timerMS = 600;

const globalState = {
  webviewPanel: undefined as vscode.WebviewPanel | undefined,
  editorPanel: undefined as vscode.TextEditor | undefined,
  editorScrollFromWebview: {
    enabled: true,
    timer: null as ReturnType<typeof setTimeout> | null,
  } as ScrollState,
  editorScrollFromBrowser: {
    enabled: true,
    timer: null as ReturnType<typeof setTimeout> | null,
  } as ScrollState,
  app: undefined as Express | undefined,
  appHtml: undefined as string | undefined,
  appHast: undefined as Root | undefined,
  wss: undefined as WebSocketServer | undefined,
  clients: new Set<WebSocket>(),
};

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
  // Preview Update Functions
  //
  // ==========================

  const updateVscodePreview = async (doc: vscode.TextDocument) => {
    if (doc.languageId !== "markdown" || !globalState.webviewPanel) return;

    const res = await renderMarkdownToHtml(
      doc.getText(),
      context,
      globalState.webviewPanel.webview
    );
    globalState.webviewPanel.webview.html = res.html;
  };

  const updateBrowserPreview = async (
    doc: vscode.TextDocument,
    fullReload = false
  ) => {
    if (doc.languageId !== "markdown" || !globalState.app) return;

    const result = await renderMarkdownToHtml(doc.getText(), context);

    if (fullReload || !globalState.appHtml || !globalState.appHast) {
      globalState.appHtml = result.html;
      globalState.appHast = result.hast;
      broadcastMessage({ type: "reload" });
    } else {
      const { editScripts, dataLineArray } = findDiff(
        globalState.appHast,
        result.hast
      );
      globalState.appHtml = result.html;
      globalState.appHast = result.hast;

      if (editScripts.length > 0) {
        broadcastMessage({ type: "edit", editScripts, dataLineArray });
      }
    }
  };

  const broadcastMessage = (message: object) => {
    if (!globalState.wss) return;
    for (const client of globalState.clients) {
      if (client.readyState === WebSocket.OPEN) {
        client.send(JSON.stringify(message));
      }
    }
  };

  // ==========================
  //
  // Browser Preview Server
  //
  // ==========================

  const startBrowserPreviewServer = (expressApp: Express) => {
    const port =
      vscode.workspace
        .getConfiguration("marknote")
        .get<number>("browserPreviewPort") ?? 3000;

    expressApp.get("/", (_req: Request, res: Response) => {
      res.send(globalState.appHtml);
    });

    expressApp.listen(port, () => {
      vscode.window.showInformationMessage(
        vscode.l10n.t(
          "The Markdown preview has opened at http://localhost:{port}.",
          { port }
        )
      );
      vscode.env.openExternal(vscode.Uri.parse(`http://localhost:${port}`));
    });

    globalState.wss = new WebSocketServer({ port: port + 1 });
    globalState.wss.on("connection", (ws) => {
      globalState.clients.add(ws);
      ws.on("close", () => globalState.clients.delete(ws));
      ws.on("message", (msg) => {
        if (typeof msg !== "string") return;
        const parsedMsg = JSON.parse(msg);

        if (parsedMsg.type === "preview-scroll") {
          if (!globalState.editorScrollFromBrowser.enabled) return;

          if (globalState.editorScrollFromWebview.timer) {
            clearTimeout(globalState.editorScrollFromWebview.timer);
          }

          globalState.editorScrollFromWebview.enabled = false;
          globalState.editorScrollFromWebview.timer = setTimeout(() => {
            globalState.editorScrollFromWebview.enabled = true;
          }, timerMS);

          const position = new vscode.Position(parsedMsg.line, 0);
          globalState.editorPanel?.revealRange(
            new vscode.Range(position, position),
            vscode.TextEditorRevealType.AtTop
          );
        }
      });
    });
  };

  // ==========================
  //
  // Register commands
  //
  // ==========================

  const registerCommands = () => {
    const { subscriptions } = context;

    subscriptions.push(
      vscode.commands.registerCommand("marknote.previewMarkdown", async () => {
        const mode =
          vscode.workspace
            .getConfiguration("marknote")
            .get<string>("defaultPreviewMode") ?? "vscode";
        if (mode === "vscode") {
          openVscodePreview();
        } else {
          await openBrowserPreview();
        }
      }),

      vscode.commands.registerCommand(
        "marknote.previewMarkdownOnVscode",
        openVscodePreview
      ),

      vscode.commands.registerCommand(
        "marknote.previewMarkdownOnBrowser",
        openBrowserPreview
      ),

      vscode.commands.registerCommand("marknote.exportHtml", () =>
        exportHtml(context)
      ),

      vscode.commands.registerCommand(
        "marknote.createWorkspaceMarknoteCss",
        () => createWorkspaceMarknoteCss(context)
      ),

      vscode.commands.registerCommand("marknote.resetGlobalMarknoteCss", () =>
        saveMarknoteCssToGlobalStorage(context)
      )
    );
  };

  const openVscodePreview = async () => {
    if (globalState.webviewPanel) return;

    const res = await previewMarkdownOnVscode(context);
    if (!res) return;

    globalState.webviewPanel = res.webviewPanel;
    globalState.editorPanel = res.editorPanel;

    globalState.webviewPanel.onDidDispose(
      () => (globalState.webviewPanel = undefined)
    );

    globalState.webviewPanel.webview.onDidReceiveMessage((msg) => {
      if (msg.type === "preview-scroll") {
        if (!globalState.editorScrollFromWebview.enabled) return;

        if (globalState.editorScrollFromBrowser.timer) {
          clearTimeout(globalState.editorScrollFromBrowser.timer);
        }

        globalState.editorScrollFromBrowser.enabled = false;
        globalState.editorScrollFromBrowser.timer = setTimeout(() => {
          globalState.editorScrollFromWebview.enabled = true;
        }, timerMS);

        const position = new vscode.Position(msg.line, 0);
        globalState.editorPanel?.revealRange(
          new vscode.Range(position, position),
          vscode.TextEditorRevealType.AtTop
        );
      }
    });
  };

  const openBrowserPreview = async () => {
    if (!globalState.app) {
      const result = await previewMarkdownOnBrowser(context);
      if (!result) return;
      ({
        app: globalState.app,
        editorPanel: globalState.editorPanel,
        html: globalState.appHtml,
        hast: globalState.appHast,
      } = result);
      startBrowserPreviewServer(globalState.app);
    } else {
      const port =
        vscode.workspace
          .getConfiguration("marknote")
          .get<number>("browserPreviewPort") ?? 3000;
      vscode.env.openExternal(vscode.Uri.parse(`http://localhost:${port}`));
    }
  };

  // ==========================
  //
  // Event Listeners
  //
  // ==========================

  const registerEventListeners = () => {
    const { subscriptions } = context;

    subscriptions.push(
      vscode.window.onDidChangeActiveTextEditor((editor) => {
        if (editor?.document.languageId === "markdown") {
          globalState.editorPanel = editor;
          updateVscodePreview(editor.document);
          updateBrowserPreview(editor.document, true);
        }
      }),

      vscode.workspace.onDidChangeTextDocument(({ document }) => {
        updateVscodePreview(document);
        updateBrowserPreview(document);
      }),

      vscode.window.onDidChangeTextEditorVisibleRanges((event) => {
        if (globalState.editorPanel !== event.textEditor) return;

        if (globalState.editorScrollFromWebview.timer) {
          clearTimeout(globalState.editorScrollFromWebview.timer);
        }
        if (globalState.editorScrollFromBrowser.timer) {
          clearTimeout(globalState.editorScrollFromBrowser.timer);
        }

        globalState.editorScrollFromWebview.enabled = false;
        globalState.editorScrollFromBrowser.enabled = false;
        globalState.editorScrollFromWebview.timer = setTimeout(() => {
          globalState.editorScrollFromWebview.enabled = true;
        }, timerMS);
        globalState.editorScrollFromBrowser.timer = setTimeout(() => {
          globalState.editorScrollFromBrowser.enabled = true;
        }, timerMS);

        const firstLine = event.visibleRanges[0]?.start.line ?? 1;
        globalState.webviewPanel?.webview.postMessage({
          type: "editor-scroll",
          line: firstLine,
        });
      })
    );
  };

  // ==========================
  //
  // Bootstrap
  //
  // ==========================

  registerCommands();
  registerEventListeners();
}

// This method is called when your extension is deactivated
export function deactivate() {}
