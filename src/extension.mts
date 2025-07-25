import { Express, Request, Response } from "express";
import { Root as HastRoot } from "hast";
import { Root as MdastRoot } from "mdast";
import * as vscode from "vscode";
import WebSocket, { WebSocketServer } from "ws";

import checkWorkspaceSettingsUpdatable from "./commands/checkWorkspaceSettingsUpdatable.mjs";
import collectAdmonitions from "./commands/collectAdmonitions.mjs";
import createNote from "./commands/createNote.mjs";
import createWorkspaceFleximarkCss from "./commands/createWorkspaceFleximarkCss.mjs";
import { isGlobalFleximarkCssExists } from "./commands/css/index.mjs";
import exportHtml from "./commands/exportHtml.mjs";
import initializeWorkspace from "./commands/initializeWorkspace.mjs";
import previewMarkdownOnBrowser from "./commands/previewMarkdownOnBrowser.mjs";
import previewMarkdownOnVscode from "./commands/previewMarkdownOnVscode.mjs";
import renderMarkdownToHtml from "./commands/renderMarkdownToHtml/index.mjs";
import saveFleximarkCssToGlobalStorage from "./commands/saveFleximarkCssToGlobalStorage.mjs";
import updateWorkspaceSettings from "./commands/updateWorkspaceSettings.mjs";
import { findDiff } from "./commands/utils/diffHTML.mjs";
import getBlockLineAndOffset from "./commands/utils/getBlockLineAndOffset.mjs";
import * as completionAbc from "./completion/completionAbc.mjs";

// Constants
const DEFAULT_PORT = 3000;
declare const __DEV__: boolean; // This is set by the esbuild process

interface GlobalExtensionState {
  mdastTree?: MdastRoot;
  plainText?: string;
  // ===============
  webviewPanel?: vscode.WebviewPanel;
  editorPanel?: vscode.TextEditor;
  isWebviewScrollProcessing: boolean;
  isBrowserScrollProcessing: boolean;
  webviewHtml?: string;
  webviewHast?: HastRoot;
  app?: Express;
  appHtml?: string;
  appHast?: HastRoot;
  wss?: WebSocketServer;
  clients: Set<WebSocket>;
}

const state: GlobalExtensionState = {
  isWebviewScrollProcessing: false,
  isBrowserScrollProcessing: false,
  clients: new Set<WebSocket>(),
};

// ---------------------------------------------
// Activation
// ---------------------------------------------
export async function activate(context: vscode.ExtensionContext) {
  console.log("Fleximark extension activated.");

  if (!isGlobalFleximarkCssExists(context)) {
    saveFleximarkCssToGlobalStorage(context);
  }

  if (await checkWorkspaceSettingsUpdatable()) {
    await updateWorkspaceSettings(context, true);
  }

  if (__DEV__) {
    const message = "📢📢📢Development mode is enabled.📢📢📢";
    vscode.window.showInformationMessage(message);
    console.log(message);

    saveFleximarkCssToGlobalStorage(context);
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    (globalThis as any).testExtensionContext = context;
  }

  registerCommands(context);
  registerEventListeners(context);
  registerCompletionProvider(context);
}

// ---------------------------------------------
// Command Registration
// ---------------------------------------------
function registerCommands(context: vscode.ExtensionContext) {
  const { subscriptions } = context;

  subscriptions.push(
    vscode.commands.registerCommand("fleximark.previewMarkdown", async () => {
      const mode =
        vscode.workspace
          .getConfiguration("fleximark")
          .get<string>("defaultPreviewMode") ?? "vscode";

      if (mode === "vscode") {
        openWebviewPreview(context);
      } else {
        openBrowserPreview(context);
      }
    }),

    vscode.commands.registerCommand("fleximark.previewMarkdownOnVscode", () =>
      openWebviewPreview(context),
    ),

    vscode.commands.registerCommand("fleximark.previewMarkdownOnBrowser", () =>
      openBrowserPreview(context),
    ),

    vscode.commands.registerCommand("fleximark.exportHtml", () =>
      exportHtml(context),
    ),

    vscode.commands.registerCommand(
      "fleximark.createWorkspaceFleximarkCss",
      () => createWorkspaceFleximarkCss(context),
    ),

    vscode.commands.registerCommand("fleximark.resetGlobalFleximarkCss", () =>
      saveFleximarkCssToGlobalStorage(context),
    ),

    vscode.commands.registerCommand("fleximark.createNote", () => createNote()),

    vscode.commands.registerCommand("fleximark.initializeWorkspace", () =>
      initializeWorkspace(context),
    ),

    vscode.commands.registerCommand(
      "fleximark.checkWorkspaceSettingsUpdatable",
      async () => {
        if (await checkWorkspaceSettingsUpdatable()) {
          await updateWorkspaceSettings(context, true);
        }
      },
    ),

    vscode.commands.registerCommand("fleximark.updateWorkspaceSettings", () =>
      updateWorkspaceSettings(context),
    ),

    vscode.commands.registerCommand("fleximark.collectAdmonitions", () =>
      collectAdmonitions(),
    ),
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
  state.webviewHtml = result.html;
  state.webviewHast = result.hast;
  state.mdastTree = result.mdast;
  state.plainText = result.plainText;

  webviewPanel.onDidDispose(() => {
    state.webviewPanel = undefined;
  });

  webviewPanel.webview.onDidReceiveMessage((msg) => {
    if (msg.type === "preview-scroll") {
      if (
        vscode.workspace
          .getConfiguration("fleximark")
          .get<boolean>("shouldSyncScroll") === false
      ) {
        return;
      }

      state.isWebviewScrollProcessing = true;

      const position = new vscode.Position(msg.line, 0);
      editorPanel?.revealRange(
        new vscode.Range(position, position),
        vscode.TextEditorRevealType.AtTop,
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
    state.mdastTree = result.mdast;
    state.plainText = result.plainText;

    startBrowserPreviewServer();
  } else {
    const port =
      vscode.workspace
        .getConfiguration("fleximark")
        .get<number>("browserPreviewPort") ?? DEFAULT_PORT;

    vscode.env.openExternal(vscode.Uri.parse(`http://localhost:${port}`));
  }
}

function startBrowserPreviewServer() {
  const app = state.app;

  if (!app) {
    vscode.window.showErrorMessage(
      vscode.l10n.t("Browser preview server is not initialized."),
    );
    return;
  }

  const port =
    vscode.workspace
      .getConfiguration("fleximark")
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
        },
      ),
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
      if (msg.type === "preview-scroll") {
        if (
          vscode.workspace
            .getConfiguration("fleximark")
            .get<boolean>("shouldSyncScroll") === false
        ) {
          return;
        }

        state.isBrowserScrollProcessing = true;

        const position = new vscode.Position(msg.line, 0);
        state.editorPanel?.revealRange(
          new vscode.Range(position, position),
          vscode.TextEditorRevealType.AtTop,
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
  context: vscode.ExtensionContext,
  fullReload = false,
) {
  if (document.languageId !== "markdown" || !state.webviewPanel) return;

  const result = await renderMarkdownToHtml(
    document.getText(),
    document.uri.fsPath,
    context,
    state.webviewPanel.webview,
  );

  if (fullReload || !state.webviewHtml || !state.webviewHast) {
    state.webviewHtml = result.html;
    state.webviewHast = result.hast;
    state.mdastTree = result.mdast;
    state.plainText = result.plainText;
    state.webviewPanel.webview.html = result.html;
  } else {
    const { editScripts, dataLineArray } = findDiff(
      state.webviewHast,
      result.hast,
    );
    state.webviewHtml = result.html;
    state.webviewHast = result.hast;
    state.mdastTree = result.mdast;
    state.plainText = result.plainText;

    if (editScripts.length > 0) {
      state.webviewPanel?.webview.postMessage({
        type: "edit",
        editScripts,
        dataLineArray,
      });
    }
  }
}

async function updateBrowserPreview(
  document: vscode.TextDocument,
  context: vscode.ExtensionContext,
  fullReload = false,
) {
  if (document.languageId !== "markdown" || !state.app) return;

  const result = await renderMarkdownToHtml(
    document.getText(),
    document.uri.fsPath,
    context,
  );

  if (fullReload || !state.appHtml || !state.appHast) {
    state.appHtml = result.html;
    state.appHast = result.hast;
    state.mdastTree = result.mdast;
    state.plainText = result.plainText;
    broadcastToClients({ type: "reload" });
  } else {
    const { editScripts, dataLineArray } = findDiff(state.appHast, result.hast);
    state.appHtml = result.html;
    state.appHast = result.hast;
    state.mdastTree = result.mdast;
    state.plainText = result.plainText;

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
        updateWebviewPreview(editor.document, context, true);
        updateBrowserPreview(editor.document, context, true);
      }
    }),

    vscode.workspace.onDidChangeTextDocument(({ document }) => {
      updateWebviewPreview(document, context);
      updateBrowserPreview(document, context);
    }),

    vscode.window.onDidChangeTextEditorSelection((e) => {
      if (e.textEditor !== state.editorPanel) return;
      if (!state.mdastTree) return;
      if (!state.plainText) return;

      const ret = getBlockLineAndOffset(
        state.mdastTree,
        state.plainText,
        e.selections[0].start.line + 1,
        e.selections[0].start.character + 1,
      );

      if (!ret) return;
      if (ret.language !== "abc") return;

      state.webviewPanel?.webview.postMessage({
        type: "cursor",
        blockType: "abc",
        lineNumber: ret.startLine,
        relativeCharNumber: ret.offsetInNode,
      });
      broadcastToClients({
        type: "cursor",
        blockType: "abc",
        lineNumber: ret.startLine,
        relativeCharNumber: ret.offsetInNode,
      });
    }),

    vscode.window.onDidChangeTextEditorVisibleRanges((event) => {
      if (event.textEditor !== state.editorPanel) return;
      if (
        vscode.workspace
          .getConfiguration("fleximark")
          .get<boolean>("shouldSyncScroll") === false
      ) {
        return;
      }

      const line = (event.visibleRanges[0]?.start.line ?? 0) + 1;

      if (state.isWebviewScrollProcessing) {
        state.isWebviewScrollProcessing = false;

        broadcastToClients({ type: "editor-scroll", line });
        return;
      }

      if (state.isBrowserScrollProcessing) {
        state.isBrowserScrollProcessing = false;

        state.webviewPanel?.webview.postMessage({
          type: "editor-scroll",
          line,
        });
        return;
      }

      state.webviewPanel?.webview.postMessage({ type: "editor-scroll", line });
      broadcastToClients({ type: "editor-scroll", line });
    }),
  );
}

// ---------------------------------------------
// Completion Provider
// ---------------------------------------------
function registerCompletionProvider(context: vscode.ExtensionContext) {
  const { subscriptions } = context;

  subscriptions.push(completionAbc.decorations, completionAbc.slashCommand);
}

// ---------------------------------------------
// Deactivation
// ---------------------------------------------
export function deactivate() {
  console.log("Fleximark extension deactivated.");
  state.webviewPanel?.dispose();
  state.wss?.close();
  state.clients.clear();
}
