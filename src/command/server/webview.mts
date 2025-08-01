import * as vscode from "vscode";

import * as fLibConvert from "../lib/convert/index.mjs";
import BaseServer from "./base.mjs";

class WebviewServer extends BaseServer {
  webviewPanel?: vscode.WebviewPanel;

  constructor() {
    super("webview");
  }

  // ---------------------------------------------
  // fLibConvert Related Methods
  // ---------------------------------------------

  protected override async convertMdToHtml(
    document: vscode.TextDocument,
    context: vscode.ExtensionContext,
    isNeedDataLineNumber?: boolean,
  ) {
    if (!this.webviewPanel) {
      throw new Error("Webview panel is not initialized.");
    }

    return await fLibConvert.convertMdToHtml({
      convertType: "webview",
      markdown: document.getText(),
      context: context,
      webview: this.webviewPanel.webview,
      markdownAbsPath: document.uri.fsPath,
      isNeedDataLineNumber: isNeedDataLineNumber ?? true,
    });
  }

  // ---------------------------------------------
  // openPreview Related Methods
  // ---------------------------------------------

  public override isPreviewOpened(): boolean {
    if (!this.webviewPanel) {
      return false;
    } else {
      return true;
    }
  }

  public override async openPreview(
    context: vscode.ExtensionContext,
  ): Promise<void> {
    if (this.webviewPanel) return;

    const document = this.getCurrentActiveTextDocument();
    if (!document) return;

    try {
      this.webviewPanel = vscode.window.createWebviewPanel(
        "markdownPreview",
        "Markdown Preview",
        vscode.ViewColumn.Beside,
        { enableScripts: true },
      );

      const result = await this.convertMdToHtml(document, context, true);
      this.registerConvertResult(result);

      this.webviewPanel.webview.html = result.html;

      this.webviewPanel.onDidDispose(() => {
        this.webviewPanel = undefined;
        this.clearConvertResult();
      });

      this.webviewPanel.webview.onDidReceiveMessage((data) => {
        const msg = JSON.parse(data.toString());
        if (msg.type === "preview-scroll") {
          this.scrollEditor(msg);
        }
      });

      return;
    } catch (error) {
      vscode.window.showErrorMessage(
        vscode.l10n.t(
          "An error occurred while preparing the Markdown preview.",
        ),
      );
      console.error(error);
      return;
    }
  }

  // ---------------------------------------------
  // updatePreview Related Methods
  // ---------------------------------------------

  protected override makeClientReload(): void {
    if (!this.webviewPanel) {
      throw new Error("Webview panel is not initialized.");
    }
    if (!this.html) {
      throw new Error("HTML content is not set.");
    }

    this.webviewPanel.webview.html = this.html;
  }

  // ---------------------------------------------
  // Public Utility Methods
  // ---------------------------------------------

  public override emitMessageToClient(message: object): void {
    this.webviewPanel?.webview.postMessage(message);
  }

  public override deactivate(): void {
    this.webviewPanel?.dispose();
  }
}

export default WebviewServer;
