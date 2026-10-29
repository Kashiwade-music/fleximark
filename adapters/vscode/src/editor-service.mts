import {
  type InitializeResult,
  SEMANTIC_TOKEN_MODIFIERS,
  SEMANTIC_TOKEN_TYPES,
  hasContractShape,
} from "@fleximark/contracts/protocol";
import * as vscode from "vscode";

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
          sourceNavigationEvents: true,
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
          tokenTypes: SEMANTIC_TOKEN_TYPES,
          tokenModifiers: SEMANTIC_TOKEN_MODIFIERS,
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
  return value;
}
