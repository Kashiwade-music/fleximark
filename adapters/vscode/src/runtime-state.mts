import * as vscode from "vscode";

import type {
  PreviewNavigationEvent,
  PreviewTarget,
  RenderFrame,
} from "../../../contracts/typescript/protocol.mjs";
import type { DaemonOrigin } from "./document-coordinator.mjs";

export interface DocumentState {
  sessionId?: string;
  version: number;
  syncing?: Promise<void>;
}

export interface PreviewBinding {
  readonly origin: DaemonOrigin;
  readonly previewSessionId: string;
  active: boolean;
  renderRevision: number;
  frame: RenderFrame;
  pendingNavigation?: PreviewNavigationEvent;
}

export interface PreviewState {
  binding: PreviewBinding;
  documentUri: string;
  sourceViewColumn?: vscode.ViewColumn;
  target: PreviewTarget;
  webviewReady?: boolean;
  messageToken?: string;
  panel?: vscode.WebviewPanel;
}

export interface DocumentRuntime {
  workspace?: vscode.WorkspaceFolder;
  documents: Map<string, DocumentState>;
  previews: Map<string, PreviewState>;
  removed: boolean;
}
