import type * as vscode from "vscode";

import { AdapterLog } from "./adapter-log.mjs";
import { CommandService } from "./command-service.mjs";
import { ConnectionRouter } from "./connection-router.mjs";
import { EditorEventForwarder } from "./editor-event-forwarder.mjs";
import { PreviewController } from "./preview-controller.mjs";
import { RuntimeManager } from "./runtime-manager.mjs";

/** Composes editor services while keeping their state in their owning component. */
export function createAdapterServices(context: vscode.ExtensionContext) {
  const logger = new AdapterLog();
  const runtime: RuntimeManager = new RuntimeManager(context, logger, {
    connected: (connection) => router.bind(connection),
    replayPreviews: (): Promise<number> => previews.replay(),
  });
  const previews: PreviewController = new PreviewController(
    runtime,
    context.extensionUri,
  );
  const router = new ConnectionRouter(previews, (error) =>
    logger.report(error),
  );
  const commands = new CommandService(runtime);
  const editor = new EditorEventForwarder(runtime, previews);
  return { runtime, previews, router, commands, editor, logger };
}

export type AdapterServices = ReturnType<typeof createAdapterServices>;
