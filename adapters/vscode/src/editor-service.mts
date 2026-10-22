import { spawn } from "node:child_process";
import * as vscode from "vscode";

import {
  type InitializeResult,
  hasContractShape,
} from "../../../contracts/typescript/protocol.mjs";
import {
  nodeReleaseManifestEnvironment,
  verifiedBundledCli,
} from "./release-manifest.mjs";
import type { JsonRpcConnection } from "./rpc.mjs";

export async function initializeEditor(
  rpc: JsonRpcConnection,
  version: string,
  workspaces: readonly vscode.WorkspaceFolder[],
): Promise<InitializeResult> {
  const response = await rpc.requestLsp<{
    capabilities: { experimental?: { fleximark?: unknown } };
  }>("initialize", {
    processId: process.pid,
    clientInfo: { name: "FlexiMark VS Code", version },
    rootUri: null,
    workspaceFolders: workspaces.map((w) => ({
      uri: w.uri.toString(),
      name: w.name,
    })),
    initializationOptions: {
      fleximark: {
        workspaces: workspaces.map((w) => ({
          uri: w.uri.toString(),
          trusted: vscode.workspace.isTrusted,
        })),
        capabilities: {
          embeddedHtml: true,
          structuredPreview: false,
          selectionEvents: true,
          viewportEvents: true,
          openExternal: true,
        },
      },
    },
    capabilities: {
      window: { showDocument: { support: true } },
      general: { positionEncodings: ["utf-16"] },
      textDocument: {
        completion: { completionItem: { snippetSupport: true } },
        semanticTokens: {
          requests: { full: true },
          tokenTypes: ["keyword", "string", "operator", "type", "property"],
          tokenModifiers: [],
          formats: ["relative"],
          overlappingTokenSupport: false,
          multilineTokenSupport: false,
        },
      },
    },
  });
  const value = response.capabilities.experimental?.fleximark;
  if (!hasContractShape("initializeResult", value))
    throw new Error("Invalid FlexiMark initialization response");
  rpc.notifyLsp("initialized", {});
  return value as InitializeResult;
}
export async function migrateWorkspace(
  extensionRoot: string,
  output: (value: string) => void,
): Promise<vscode.WorkspaceFolder | undefined> {
  const selected = await vscode.window.showQuickPick(
    (vscode.workspace.workspaceFolders ?? []).map((workspace) => ({
      label: workspace.name,
      workspace,
    })),
    { placeHolder: "Select the workspace to migrate" },
  );
  if (!selected) return;
  const binary = await verifiedBundledCli(
    nodeReleaseManifestEnvironment(extensionRoot),
  );
  await new Promise<void>((resolve, reject) => {
    const child = spawn(binary, ["migrate", selected.workspace.uri.fsPath], {
      cwd: selected.workspace.uri.fsPath,
      windowsHide: true,
      stdio: ["ignore", "pipe", "pipe"],
    });
    child.stdout.on("data", (data: Buffer) => output(data.toString()));
    child.stderr.on("data", (data: Buffer) => output(data.toString()));
    child.once("error", reject);
    child.once("exit", (code) =>
      code === 0
        ? resolve()
        : reject(new Error("Workspace migration failed (" + code + ")")),
    );
  });
  return selected.workspace;
}
