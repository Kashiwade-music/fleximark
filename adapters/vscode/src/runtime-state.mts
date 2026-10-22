import * as vscode from "vscode";

import type { PreviewTarget } from "../../../contracts/typescript/protocol.mjs";
import type { DaemonOrigin } from "./document-coordinator.mjs";

export interface DocumentState {
  sessionId?: string;
  version: number;
  syncing?: Promise<void>;
  checkpoint?: NodeJS.Timeout;
}

export interface PreviewState {
  origin: DaemonOrigin;
  documentUri: string;
  sourceViewColumn?: vscode.ViewColumn;
  previewSessionId: string;
  remoteSessionActive: boolean;
  target: PreviewTarget;
  renderRevision: number;
  notifiedRevision: number;
  webviewReady?: boolean;
  readInFlight?: Promise<void>;
  readAgain?: boolean;
  forceRead?: boolean;
  messageToken?: string;
  panel?: vscode.WebviewPanel;
}

export interface WorkspaceRuntime {
  workspace: vscode.WorkspaceFolder;
  documents: Map<string, DocumentState>;
  previews: Map<string, PreviewState>;
  removed: boolean;
}
