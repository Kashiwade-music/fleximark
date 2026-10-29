import * as vscode from "vscode";

import type { AdapterServices } from "./adapter-services.mjs";

export interface AdapterRecoveryState {
  connectionGeneration: number;
  daemonInstanceId?: string;
  documentSessions: Record<string, string | undefined>;
  previewSessions: Record<string, string>;
  previewRenderRevisions: Record<string, number | undefined>;
}

export interface FlexiMarkTestApi {
  crashDaemon(): void;
  recoveryState(): AdapterRecoveryState;
  openExternalPreview(): Promise<string>;
}

/** Builds the extension test API only for an explicit editor test activation. */
export function createExtensionTestApi(
  context: vscode.ExtensionContext,
  services: AdapterServices,
): FlexiMarkTestApi | undefined {
  if (context.extensionMode !== vscode.ExtensionMode.Test) return;
  return {
    crashDaemon: () => {
      services.runtime.supervisor.process?.kill();
    },
    recoveryState: () => ({
      connectionGeneration: services.runtime.supervisor.connectionGeneration,
      daemonInstanceId: services.runtime.connection?.daemonInstanceId,
      documentSessions: Object.fromEntries(
        [...services.runtime.runtimes.values()].flatMap((runtime) =>
          [...runtime.documents].map(([uri, state]) => [uri, state.sessionId]),
        ),
      ),
      previewSessions: Object.fromEntries(
        [...services.runtime.runtimes.values()].flatMap((runtime) =>
          [...runtime.previews.values()].map((preview) => [
            preview.documentUri,
            preview.binding.previewSessionId,
          ]),
        ),
      ),
      previewRenderRevisions: Object.fromEntries(
        [...services.runtime.runtimes.values()].flatMap((runtime) =>
          [...runtime.previews.values()].map((preview) => [
            preview.documentUri,
            preview.binding.renderRevision || undefined,
          ]),
        ),
      ),
    }),
    openExternalPreview: async () => {
      const editor = vscode.window.activeTextEditor;
      const document = editor?.document;
      if (!document) throw new Error("No active document");
      await services.runtime.activateDocument(document);
      const runtime = services.runtime.runtimeForDocument(document);
      const preview =
        runtime &&
        (await services.previews.request(runtime, document, "externalBrowser"));
      if (!preview || preview.target !== "externalBrowser")
        throw new Error("External preview was not created");
      preview.sourceViewColumn = editor?.viewColumn;
      return preview.url;
    },
  };
}
