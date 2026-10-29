import type { PreviewTarget } from "@fleximark/contracts/protocol";
import * as vscode from "vscode";

import { createAdapterServices } from "./adapter-services.mjs";
import { FlexiMarkAdapter } from "./adapter.mjs";
import { registerCommands } from "./commands.mjs";
import { registerEditorEvents } from "./events.mjs";
import {
  type FlexiMarkTestApi,
  createExtensionTestApi,
} from "./extension-test-api.mjs";
import { registerProviders } from "./providers.mjs";

let adapter: FlexiMarkAdapter | undefined;

export type { FlexiMarkTestApi } from "./extension-test-api.mjs";

export async function activate(
  context: vscode.ExtensionContext,
): Promise<FlexiMarkTestApi | undefined> {
  const services = createAdapterServices(context);
  adapter = new FlexiMarkAdapter(context, services);
  const activatedAdapter = adapter;
  const isActive = () => adapter === activatedAdapter;
  context.subscriptions.push(activatedAdapter);
  context.subscriptions.push(
    ...registerProviders(activatedAdapter, undefined, isActive),
    ...registerCommands(activatedAdapter, undefined, isActive),
  );

  context.subscriptions.push(
    ...registerEditorEvents(activatedAdapter, undefined, isActive),
  );

  for (const workspace of vscode.workspace.workspaceFolders ?? []) {
    if (!isActive()) return;
    void activatedAdapter.start(workspace).catch((error: unknown) => {
      if (isActive()) activatedAdapter.report(error);
    });
  }

  void activatedAdapter
    .activateEditor(vscode.window.activeTextEditor)
    .then(() => {
      if (adapter !== activatedAdapter) return;
      const config = vscode.workspace.getConfiguration("fleximark");
      if (
        config.get<boolean>("autoOpenPreview", false) &&
        vscode.window.activeTextEditor?.document.languageId === "markdown"
      ) {
        return activatedAdapter.openPreview(
          config.get<PreviewTarget>("previewTarget", "embeddedHtml"),
        );
      }
    })
    .catch((error: unknown) => {
      if (adapter === activatedAdapter) activatedAdapter.report(error);
    });

  if (context.extensionMode === vscode.ExtensionMode.Test) {
    return createExtensionTestApi(context, services);
  }
}

export function deactivate(): void {
  adapter?.dispose();
  adapter = undefined;
}
