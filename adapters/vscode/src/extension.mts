import * as vscode from "vscode";

import { type AdapterRecoveryState, FlexiMarkAdapter } from "./adapter.mjs";
import { registerCommands } from "./commands.mjs";
import { registerEditorEvents } from "./events.mjs";
import { registerProviders } from "./providers.mjs";

let adapter: FlexiMarkAdapter | undefined;

export interface FlexiMarkTestApi {
  crashDaemon(): void;
  recoveryState(): AdapterRecoveryState;
}

export function activate(
  context: vscode.ExtensionContext,
): FlexiMarkTestApi | undefined {
  adapter = new FlexiMarkAdapter(context);
  const activatedAdapter = adapter;
  const isActive = () => adapter === activatedAdapter;
  context.subscriptions.push(activatedAdapter);
  context.subscriptions.push(
    ...registerProviders(activatedAdapter, undefined, isActive),
    ...registerCommands(activatedAdapter, undefined, isActive),
    ...registerEditorEvents(activatedAdapter, undefined, isActive),
  );

  void activatedAdapter
    .activateDocument(vscode.window.activeTextEditor?.document)
    .then(() => {
      if (adapter !== activatedAdapter) return;
      const config = vscode.workspace.getConfiguration("fleximark");
      if (
        config.get<boolean>("autoOpenPreview", false) &&
        vscode.window.activeTextEditor?.document.languageId === "markdown"
      ) {
        return activatedAdapter.openPreview(
          config.get<"embeddedHtml" | "externalBrowser">(
            "previewTarget",
            "embeddedHtml",
          ),
        );
      }
    })
    .catch((error: unknown) => {
      if (adapter === activatedAdapter) activatedAdapter.report(error);
    });

  if (context.extensionMode === vscode.ExtensionMode.Test) {
    return {
      crashDaemon: () => adapter?.crashDaemonForTest(),
      recoveryState: () => {
        if (!adapter) throw new Error("FlexiMark adapter is unavailable");
        return adapter.recoveryStateForTest();
      },
    };
  }
}

export function deactivate(): void {
  adapter?.dispose();
  adapter = undefined;
}
