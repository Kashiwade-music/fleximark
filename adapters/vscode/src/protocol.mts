import type {
  NavigationEntry,
  RenderPublication,
  RenderSnapshot,
} from "../../../web/preview-client/index.mjs";
import type {
  EditorNavigationEvent,
  PreviewNavigationEvent,
} from "../../../web/preview-client/navigation.mjs";

export const protocolVersion = 1;

export type PreviewTarget = "embeddedHtml" | "externalBrowser";

export interface InitializeResult {
  protocolVersion: number;
  daemonInstanceId: string;
  workspaceStatuses: { uri: string; enabled: boolean; error?: string }[];
  capabilities: {
    htmlRender: boolean;
    documentCheckpoint: boolean;
    selectionEvents: boolean;
    viewportEvents: boolean;
    workspaceCommands: string[];
  };
}

export interface AttachDocumentResult {
  documentSessionId: string;
  documentVersion: number;
  contentHash: string;
}

export interface CreatePreviewResult {
  previewSessionId: string;
  url?: string;
  initialPublication: RenderSnapshot;
}

export interface CommandResult {
  message?: { level: "info" | "warning" | "error"; text: string };
  openUri?: string;
}

export interface ExecuteCommandParams {
  daemonInstanceId: string;
  command: string;
  documentSessionId?: string;
  expectedDocumentVersion?: number;
  workspaceUri?: string;
  destinationUri?: string;
  noteCategory?: string;
  noteTemplate?: string;
}

export interface GetNoteOptionsParams {
  daemonInstanceId: string;
  workspaceUri: string;
}

export interface GetNoteOptionsResult {
  categories: string[];
  templates: string[];
}

export interface PreviewEvent {
  daemonInstanceId: string;
  previewSessionId: string;
  renderRevision: number;
  event: RenderPublication | PreviewNavigationEvent | SourceNavigationEvent;
}

export type SourceRange = NavigationEntry["sourceRange"];

export type SourceNavigationEvent =
  | { type: "selectSource"; sourceRange: SourceRange }
  | { type: "revealSource"; sourceRange: SourceRange };

export type PreviewClientEvent = EditorNavigationEvent;

export interface RequestFullTextParams {
  daemonInstanceId: string;
  uri: string;
  documentSessionId: string;
  reason: string;
}
