import * as vscode from "vscode";

import * as fCommand from "./command/command/index.mjs";
import * as fLibCss from "./command/lib/css/index.mjs";
import * as fLibFs from "./command/lib/fs/index.mjs";
import * as fLibUnist from "./command/lib/unist/index.mjs";
import * as fShared from "./command/shared/index.mjs";
import * as completionAbc from "./completion/completionAbc.mjs";

// ---------------------------------------------
// Constants
// ---------------------------------------------

declare const __DEV__: boolean; // This is set by the esbuild process
const state = new fShared.State();

// ---------------------------------------------
// Activation
// ---------------------------------------------

export async function activate(context: vscode.ExtensionContext) {
  if (await fLibFs.isFleximarkWorkspace()) {
    if (!fLibCss.isGlobalFleximarkCssExists(context)) {
      fCommand.resetGlobalFleximarkCss(context);
    }

    if (await fCommand.checkWorkspaceSettingsUpdatable()) {
      await fCommand.updateWorkspaceSettings(context, true);
    }
  }

  if (__DEV__) {
    const message = "📢📢📢Development mode is enabled.📢📢📢";
    vscode.window.showInformationMessage(message);
    console.log(message);

    fCommand.resetGlobalFleximarkCss(context);
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
        state.webviewServer.openPreview(context);
      } else {
        state.browserServer.openPreview(context);
      }
    }),

    vscode.commands.registerCommand("fleximark.previewMarkdownOnVscode", () =>
      state.webviewServer.openPreview(context),
    ),

    vscode.commands.registerCommand("fleximark.previewMarkdownOnBrowser", () =>
      state.browserServer.openPreview(context),
    ),

    vscode.commands.registerCommand("fleximark.exportHtml", () =>
      fCommand.exportHtml(context),
    ),

    vscode.commands.registerCommand(
      "fleximark.createWorkspaceFleximarkCss",
      () => fCommand.createWorkspaceFleximarkCss(context),
    ),

    vscode.commands.registerCommand("fleximark.resetGlobalFleximarkCss", () =>
      fCommand.resetGlobalFleximarkCss(context),
    ),

    vscode.commands.registerCommand("fleximark.createNote", () =>
      fCommand.createNote(),
    ),

    vscode.commands.registerCommand("fleximark.initializeWorkspace", () =>
      fCommand.initializeWorkspace(context),
    ),

    vscode.commands.registerCommand(
      "fleximark.checkWorkspaceSettingsUpdatable",
      async () => {
        if (await fCommand.checkWorkspaceSettingsUpdatable()) {
          await fCommand.updateWorkspaceSettings(context, true);
        }
      },
    ),

    vscode.commands.registerCommand("fleximark.updateWorkspaceSettings", () =>
      fCommand.updateWorkspaceSettings(context),
    ),

    vscode.commands.registerCommand("fleximark.collectAdmonitions", () =>
      fCommand.collectAdmonitions(),
    ),
  );
}

// ---------------------------------------------
// Event Listeners
// ---------------------------------------------

function registerEventListeners(context: vscode.ExtensionContext) {
  const { subscriptions } = context;

  subscriptions.push(
    vscode.window.onDidChangeActiveTextEditor((editor) => {
      if (editor?.document.languageId === "markdown") {
        state.browserServer.updatePreview(editor.document, context, true);
        state.webviewServer.updatePreview(editor.document, context, true);
      }
    }),

    vscode.workspace.onDidChangeTextDocument(({ document }) => {
      state.browserServer.updatePreview(document, context, false);
      state.webviewServer.updatePreview(document, context, false);
    }),

    vscode.window.onDidChangeTextEditorSelection((e) => {
      if (e.textEditor !== state.getEditorPanel()) return;

      const mdast = state.getMdast();
      if (!mdast) return;
      const plainText = state.getPlainText();
      if (!plainText) return;

      const ret = fLibUnist.getBlockLineAndOffset(
        mdast,
        plainText,
        e.selections[0].start.line + 1,
        e.selections[0].start.character + 1,
      );

      if (!ret) return;
      if (ret.language !== "abc") return;

      const message = {
        type: "cursor",
        blockType: "abc",
        lineNumber: ret.startLine,
        relativeCharNumber: ret.offsetInNode,
      };

      state.browserServer.emitMessageToClient(message);
      state.webviewServer.emitMessageToClient(message);
    }),

    vscode.window.onDidChangeTextEditorVisibleRanges((event) => {
      if (event.textEditor !== state.getEditorPanel()) return;
      if (
        vscode.workspace
          .getConfiguration("fleximark")
          .get<boolean>("shouldSyncScroll") === false
      ) {
        return;
      }

      const line = (event.visibleRanges[0]?.start.line ?? 0) + 1;

      if (state.webviewServer.isScrollProcessing) {
        state.webviewServer.isScrollProcessing = false;

        state.browserServer.emitMessageToClient({
          type: "editor-scroll",
          line,
        });
        return;
      }

      if (state.browserServer.isScrollProcessing) {
        state.browserServer.isScrollProcessing = false;

        state.webviewServer.emitMessageToClient({
          type: "editor-scroll",
          line,
        });
        return;
      }

      state.webviewServer.emitMessageToClient({ type: "editor-scroll", line });
      state.browserServer.emitMessageToClient({ type: "editor-scroll", line });
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
  state.webviewServer.deactivate();
  state.browserServer.deactivate();
}
