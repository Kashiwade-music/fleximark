import type { RenderFrame } from "@fleximark/contracts/protocol";
import type { PreviewSession } from "@fleximark/preview/session";
import type * as vscode from "vscode";

import {
  ConnectionBinding,
  type DaemonConnection,
} from "./daemon-connection.mjs";

export interface DocumentState {
  sessionId?: string;
  version: number;
  syncing?: Promise<void>;
}

/** A preview lease checks connection and membership in exactly one place. */
export class PreviewBinding extends ConnectionBinding {
  active = true;
  renderRevision = 0;
  constructor(
    connection: DaemonConnection,
    readonly previewSessionId: string,
    current: () => boolean,
  ) {
    super(connection, current);
  }
  override isCurrent(connection = this.connection): boolean {
    return this.active && super.isCurrent(connection);
  }
}

export class EmbeddedBinding extends PreviewBinding {
  delivery?: PreviewSession;
  constructor(
    connection: DaemonConnection,
    frame: RenderFrame,
    current: () => boolean,
  ) {
    super(connection, frame.previewSessionId, current);
    this.frame = frame;
  }
  frame: RenderFrame;
}

interface PreviewDocument {
  documentUri: string;
  sourceViewColumn?: vscode.ViewColumn;
}

export interface EmbeddedPreview extends PreviewDocument {
  target: "embeddedHtml";
  binding: EmbeddedBinding;
  webviewReady?: boolean;
  messageToken?: string;
  panel?: vscode.WebviewPanel;
}

export interface ExternalPreview extends PreviewDocument {
  target: "externalBrowser";
  binding: PreviewBinding;
  url: string;
}

export type PreviewState = EmbeddedPreview | ExternalPreview;

export interface DocumentRuntime {
  workspace?: vscode.WorkspaceFolder;
  documents: Map<string, DocumentState>;
  previews: Map<string, PreviewState>;
  removed: boolean;
}
