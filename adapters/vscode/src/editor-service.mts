import * as vscode from "vscode";

import {
  type InitializeResult,
  hasContractShape,
} from "../../../contracts/typescript/protocol.mjs";
import type { JsonRpcConnection } from "./rpc.mjs";

export function synchronizeEditorWorkspaces(
  rpc: JsonRpcConnection,
  previous: Set<string>,
  folders: readonly vscode.WorkspaceFolder[],
): void {
  const current = new Set(folders.map((folder) => folder.uri.toString()));
  const added = folders
    .filter((folder) => !previous.has(folder.uri.toString()))
    .map((folder) => ({ uri: folder.uri.toString(), name: folder.name }));
  const removed = [...previous]
    .filter((uri) => !current.has(uri))
    .map((uri) => ({ uri, name: "" }));
  if (added.length || removed.length)
    rpc.notifyLsp("workspace/didChangeWorkspaceFolders", {
      event: { added, removed },
    });
  previous.clear();
  for (const uri of current) previous.add(uri);
}

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
          selectionEvents: true,
          viewportEvents: true,
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
    throw new Error(vscode.l10n.t("Invalid FlexiMark initialization response"));
  rpc.notifyLsp("initialized", {});
  return value as InitializeResult;
}
