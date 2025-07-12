import * as vscode from "vscode";
import previewMarkdownOnVscode from "./commands/previewMarkdownOnVscode.mjs";
import previewMarkdownOnBrowser from "./commands/previewMarkdownOnBrowser.mjs";
import exportHtml from "./commands/exportHtml.mjs";
import createWorkspaceMarknoteCss from "./commands/createWorkspaceMarknoteCss.mjs";
import saveMarknoteCssToGlobalStorage from "./commands/saveMarknoteCssToGlobalStorage.mjs";
import renderMarkdownToHtml from "./commands/renderMarkdownToHtml/index.mjs";
import { isGlobalMarknoteCssExists } from "./commands/css/index.mjs";
import { findDiff } from "./commands/utils/diffHTML.mjs";

import express, { Express, Request, Response } from "express";
import WebSocket, { WebSocketServer } from "ws";
import { Root } from "hast";

// Constants
const SCROLL_THROTTLE_MS = 300;
const DEFAULT_PORT = 3000;

// State Interfaces
interface ScrollState {
  enabled: boolean;
  timer: ReturnType<typeof setTimeout> | null;
}

interface GlobalExtensionState {
  webviewPanel?: vscode.WebviewPanel;
  editorPanel?: vscode.TextEditor;
  editorScrollFromWebview: ScrollState;
  editorScrollFromBrowser: ScrollState;
  app?: Express;
  appHtml?: string;
  appHast?: Root;
  wss?: WebSocketServer;
  clients: Set<WebSocket>;
}

const state: GlobalExtensionState = {
  editorScrollFromWebview: { enabled: true, timer: null },
  editorScrollFromBrowser: { enabled: true, timer: null },
  clients: new Set<WebSocket>(),
};

// ---------------------------------------------
// Activation
// ---------------------------------------------
export function activate(context: vscode.ExtensionContext) {
  console.log("Marknote extension activated.");

  if (!isGlobalMarknoteCssExists(context)) {
    saveMarknoteCssToGlobalStorage(context);
  }

  // if in Dev, comment out the next line
  saveMarknoteCssToGlobalStorage(context);

  registerCommands(context);
  registerEventListeners(context);
}

// ---------------------------------------------
// Command Registration
// ---------------------------------------------
function registerCommands(context: vscode.ExtensionContext) {
  const { subscriptions } = context;

  subscriptions.push(
    vscode.commands.registerCommand("marknote.previewMarkdown", async () => {
      const mode =
        vscode.workspace
          .getConfiguration("marknote")
          .get<string>("defaultPreviewMode") ?? "vscode";

      mode === "vscode"
        ? openWebviewPreview(context)
        : openBrowserPreview(context);
    }),

    vscode.commands.registerCommand("marknote.previewMarkdownOnVscode", () =>
      openWebviewPreview(context)
    ),

    vscode.commands.registerCommand("marknote.previewMarkdownOnBrowser", () =>
      openBrowserPreview(context)
    ),

    vscode.commands.registerCommand("marknote.exportHtml", () =>
      exportHtml(context)
    ),

    vscode.commands.registerCommand("marknote.createWorkspaceMarknoteCss", () =>
      createWorkspaceMarknoteCss(context)
    ),

    vscode.commands.registerCommand("marknote.resetGlobalMarknoteCss", () =>
      saveMarknoteCssToGlobalStorage(context)
    )
  );
}

// ---------------------------------------------
// Webview Preview
// ---------------------------------------------
async function openWebviewPreview(context: vscode.ExtensionContext) {
  if (state.webviewPanel) return;

  const result = await previewMarkdownOnVscode(context);
  if (!result) return;

  const { webviewPanel, editorPanel } = result;

  state.webviewPanel = webviewPanel;
  state.editorPanel = editorPanel;

  webviewPanel.onDidDispose(() => {
    state.webviewPanel = undefined;
  });

  webviewPanel.webview.onDidReceiveMessage((msg) => {
    if (
      msg.type === "preview-scroll" &&
      state.editorScrollFromWebview.enabled
    ) {
      const position = new vscode.Position(msg.line, 0);
      editorPanel?.revealRange(
        new vscode.Range(position, position),
        vscode.TextEditorRevealType.AtTop
      );
    }
  });
}

// ---------------------------------------------
// Browser Preview
// ---------------------------------------------
async function openBrowserPreview(context: vscode.ExtensionContext) {
  if (!state.app) {
    const result = await previewMarkdownOnBrowser(context);
    if (!result) return;

    state.app = result.app;
    state.editorPanel = result.editorPanel;
    state.appHtml = result.html;
    state.appHast = result.hast;

    startBrowserPreviewServer(context);
  }

  const port =
    vscode.workspace
      .getConfiguration("marknote")
      .get<number>("browserPreviewPort") ?? DEFAULT_PORT;

  vscode.env.openExternal(vscode.Uri.parse(`http://localhost:${port}`));
}

function startBrowserPreviewServer(context: vscode.ExtensionContext) {
  const app = state.app!;
  const port =
    vscode.workspace
      .getConfiguration("marknote")
      .get<number>("browserPreviewPort") ?? DEFAULT_PORT;

  app.get("/", (_req: Request, res: Response) => {
    res.send(state.appHtml);
  });

  app.listen(port, () => {
    vscode.window.showInformationMessage(
      vscode.l10n.t(
        "The Markdown preview has opened at http://localhost:{port}.",
        {
          port,
        }
      )
    );
    vscode.env.openExternal(vscode.Uri.parse(`http://localhost:${port}`));
  });

  const wss = new WebSocketServer({ port: port + 1 });
  state.wss = wss;

  wss.on("connection", (ws) => {
    state.clients.add(ws);
    ws.on("close", () => state.clients.delete(ws));

    ws.on("message", (data) => {
      const msg = JSON.parse(data.toString());
      if (
        msg.type === "preview-scroll" &&
        state.editorScrollFromBrowser.enabled
      ) {
        const position = new vscode.Position(msg.line, 0);
        state.editorPanel?.revealRange(
          new vscode.Range(position, position),
          vscode.TextEditorRevealType.AtTop
        );
      }
    });
  });
}

// ---------------------------------------------
// Live Preview Updates
// ---------------------------------------------
async function updateWebviewPreview(
  document: vscode.TextDocument,
  context: vscode.ExtensionContext
) {
  if (document.languageId !== "markdown" || !state.webviewPanel) return;

  const result = await renderMarkdownToHtml(
    document.getText(),
    context,
    state.webviewPanel.webview
  );
  state.webviewPanel.webview.html = result.html;
}

async function updateBrowserPreview(
  document: vscode.TextDocument,
  context: vscode.ExtensionContext,
  fullReload = false
) {
  if (document.languageId !== "markdown" || !state.app) return;

  const result = await renderMarkdownToHtml(document.getText(), context);

  if (fullReload || !state.appHtml || !state.appHast) {
    state.appHtml = result.html;
    state.appHast = result.hast;
    broadcastToClients({ type: "reload" });
  } else {
    const { editScripts, dataLineArray } = findDiff(state.appHast, result.hast);
    state.appHtml = result.html;
    state.appHast = result.hast;

    if (editScripts.length > 0) {
      broadcastToClients({ type: "edit", editScripts, dataLineArray });
    }
  }
}

function broadcastToClients(message: object) {
  state.wss?.clients.forEach((client) => {
    if (client.readyState === WebSocket.OPEN) {
      client.send(JSON.stringify(message));
    }
  });
}

// ---------------------------------------------
// Event Listeners
// ---------------------------------------------
function registerEventListeners(context: vscode.ExtensionContext) {
  const { subscriptions } = context;

  subscriptions.push(
    vscode.window.onDidChangeActiveTextEditor((editor) => {
      if (editor?.document.languageId === "markdown") {
        state.editorPanel = editor;
        updateWebviewPreview(editor.document, context);
        updateBrowserPreview(editor.document, context, true);
      }
    }),

    vscode.workspace.onDidChangeTextDocument(({ document }) => {
      updateWebviewPreview(document, context);
      updateBrowserPreview(document, context);
    }),

    vscode.window.onDidChangeTextEditorVisibleRanges((event) => {
      if (event.textEditor !== state.editorPanel) return;

      clearTimeout(state.editorScrollFromWebview.timer!);
      clearTimeout(state.editorScrollFromBrowser.timer!);

      state.editorScrollFromWebview.enabled = false;
      state.editorScrollFromBrowser.enabled = false;

      state.editorScrollFromWebview.timer = setTimeout(() => {
        state.editorScrollFromWebview.enabled = true;
      }, SCROLL_THROTTLE_MS);

      state.editorScrollFromBrowser.timer = setTimeout(() => {
        state.editorScrollFromBrowser.enabled = true;
      }, SCROLL_THROTTLE_MS);

      const line = event.visibleRanges[0]?.start.line ?? 0;

      state.webviewPanel?.webview.postMessage({ type: "editor-scroll", line });
      broadcastToClients({ type: "editor-scroll", line });
    })
  );
}

// ---------------------------------------------
// Deactivation
// ---------------------------------------------
export function deactivate() {}
