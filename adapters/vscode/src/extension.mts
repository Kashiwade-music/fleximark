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

export async function activate(
  context: vscode.ExtensionContext,
): Promise<FlexiMarkTestApi | undefined> {
  adapter = new FlexiMarkAdapter(context);
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
