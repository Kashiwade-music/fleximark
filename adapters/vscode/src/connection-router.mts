import type { JsonRpcMessageEnvelope } from "@fleximark/contracts/protocol";
import * as vscode from "vscode";

import type { DaemonConnection } from "./daemon-connection.mjs";
import { applyPublishedDiagnostics } from "./diagnostics.mjs";
import type { PreviewController } from "./preview-controller.mjs";
import { isShowDocumentParams, showDocument } from "./show-document.mjs";

/** Routes the typed notifications validated once by the RPC boundary. */
export class ConnectionRouter {
  readonly #diagnostics =
    vscode.languages.createDiagnosticCollection("fleximark");
  constructor(
    private readonly previews: PreviewController,
    private readonly report: (error: unknown) => void,
  ) {}

  bind(connection: DaemonConnection): void {
    connection.rpc.onNotification("fleximark/previewChanged", (params) => {
      if (!connection.signal.aborted) this.previews.frame(connection, params);
    });
    connection.rpc.onNotification("fleximark/previewEvent", (params) => {
      if (!connection.signal.aborted)
        void this.previews.event(connection, params).catch(this.report);
    });
    connection.rpc.onNotification(
      "textDocument/publishDiagnostics",
      (params) => {
        if (!connection.signal.aborted)
          applyPublishedDiagnostics(this.#diagnostics, params);
      },
    );
    connection.rpc.on("request", (message: JsonRpcMessageEnvelope) => {
      void this.request(connection, message).catch(this.report);
    });
  }

  private async request(
    connection: DaemonConnection,
    message: JsonRpcMessageEnvelope,
  ): Promise<void> {
    if (
      connection.signal.aborted ||
      message.id === undefined ||
      message.method !== "window/showDocument"
    )
      return;
    if (!isShowDocumentParams(message.params)) {
      connection.rpc.respond(message.id, undefined, {
        code: -32602,
        message: "Invalid showDocument parameters",
      });
      return;
    }
    try {
      const success = await showDocument(message.params);
      if (!connection.signal.aborted)
        connection.rpc.respond(message.id, { success });
    } catch {
      if (!connection.signal.aborted)
        connection.rpc.respond(message.id, { success: false });
    }
  }

  dispose(): void {
    this.#diagnostics.dispose();
  }
}
