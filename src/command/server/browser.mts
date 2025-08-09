import express, { Express, Request, Response } from "express";
import * as vscode from "vscode";
import WebSocket, { WebSocketServer } from "ws";

import * as fLibConvert from "../lib/convert/convertMdToHtml/index.mjs";
import BaseServer from "./base.mjs";

const DEFAULT_PORT = 3000;

class BrowserServer extends BaseServer {
  app?: Express;
  wss?: WebSocketServer;
  clients = new Set<WebSocket>();

  constructor() {
    super("browser");
  }

  private getPort(): number {
    return (
      vscode.workspace
        .getConfiguration("fleximark")
        .get<number>("browserPreviewPort") ?? DEFAULT_PORT
    );
  }

  // ---------------------------------------------
  // fLibConvert Related Methods
  // ---------------------------------------------

  protected override async convertMdToHtml(
    document: vscode.TextDocument,
    context: vscode.ExtensionContext,
    isNeedDataLineNumber?: boolean,
  ) {
    if (!this.app) {
      throw new Error("Browser server is not initialized.");
    }

    return await fLibConvert.convertMdToHtml({
      convertType: "browser",
      markdown: document.getText(),
      context: context,
      isNeedDataLineNumber: isNeedDataLineNumber ?? true,
      app: this.app,
      markdownAbsPath: document.uri.fsPath,
    });
  }

  // ---------------------------------------------
  // openPreview Related Methods
  // ---------------------------------------------

  public override isPreviewOpened(): boolean {
    if (!this.app) {
      return false;
    } else {
      return true;
    }
  }

  public override async openPreview(
    context: vscode.ExtensionContext,
  ): Promise<void> {
    if (this.app) {
      const port = this.getPort();
      vscode.env.openExternal(vscode.Uri.parse(`http://localhost:${port}`));
      return;
    }

    const document = this.getCurrentActiveTextDocument();
    if (!document) return;

    const workspaceFolder = vscode.workspace.getWorkspaceFolder(document.uri);
    if (!workspaceFolder) {
      vscode.window.showErrorMessage(
        vscode.l10n.t("The Markdown file must be in a workspace folder."),
      );
      return;
    }

    try {
      this.app = express();
      this.app.use(express.static(context.extensionPath));
      this.app.use(express.static(workspaceFolder.uri.fsPath));

      const result = await this.convertMdToHtml(document, context, true);
      this.registerConvertResult(result);

      const port = this.getPort();

      this.app.get("/", (_req: Request, res: Response) => {
        res.send(this.html);
      });

      this.app.listen(port, () => {
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

      this.wss = new WebSocketServer({ port: port + 1 });

      this.wss.on("connection", (ws) => {
        this.clients.add(ws);

        ws.on("close", () => this.clients.delete(ws));

        ws.on("message", (data) => {
          const msg = JSON.parse(data.toString());
          if (msg.type === "preview-scroll") {
            this.scrollEditor(msg);
          }
        });
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
    this.emitMessageToClient({
      type: "reload",
    });
  }

  // ---------------------------------------------
  // Public Utility Methods
  // ---------------------------------------------

  public override emitMessageToClient(message: object): void {
    this.wss?.clients.forEach((client) => {
      if (client.readyState === WebSocket.OPEN) {
        client.send(JSON.stringify(message));
      }
    });
  }

  public override deactivate(): void {
    this.initializeBaseMembers();
    this.wss?.close();
    this.clients.clear();
  }
}

export default BrowserServer;
