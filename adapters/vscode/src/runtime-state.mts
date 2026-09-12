import * as vscode from "vscode";

import type { RenderPublication } from "../../../web/preview-client/index.mjs";
import type { PreviewTarget } from "./protocol.mjs";
import type { JsonRpcConnection } from "./rpc.mjs";

export interface DocumentState {
  sessionId?: string;
  version: number;
  syncing?: Promise<void>;
  checkpoint?: NodeJS.Timeout;
}

export interface PreviewState {
  originRpc: JsonRpcConnection;
  originDaemonInstanceId: string;
  originGeneration: number;
  documentUri: string;
  sourceViewColumn?: vscode.ViewColumn;
  previewSessionId: string;
  target: PreviewTarget;
  url?: string;
  initialPublication: RenderPublication;
  renderRevision: number;
  renderedRevision?: number;
  ready?: boolean;
  handshakeEpoch?: number;
  handshakeInFlight?: Promise<void>;
  handshakePending?: boolean;
  reloadPending?: boolean;
  messageToken?: string;
  panel?: vscode.WebviewPanel;
}

export interface WorkspaceRuntime {
  workspace: vscode.WorkspaceFolder;
  documents: Map<string, DocumentState>;
  previews: Map<string, PreviewState>;
  removed: boolean;
}
