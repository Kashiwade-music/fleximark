import * as path from "node:path";
import type * as vscode from "vscode";

import type { DaemonOrigin } from "./document-coordinator.mjs";
import {
  type CreatePreviewResult,
  type EditorNavigationEvent,
  type PreviewEvent,
  type PreviewTarget,
  type SourceNavigationEvent,
  isWebviewInboundMessage,
  shouldForwardEditorNavigation,
} from "./protocol.mjs";
import type {
  DocumentState,
  PreviewState,
  WorkspaceRuntime,
} from "./runtime-state.mjs";
import { previewEventAction } from "./workspace-selection.mjs";

export interface PreviewCandidateOrigin {
  readonly origin: DaemonOrigin;
  readonly runtime: WorkspaceRuntime;
  readonly documentUri: string;
  readonly documentState: DocumentState;
  readonly documentSessionId: string;
  readonly documentVersion: number;
}

export interface PreviewCandidate extends PreviewCandidateOrigin {
  readonly result: CreatePreviewResult;
}

export interface PreviewCandidateCurrentState {
  readonly disposed: boolean;
  readonly runtime: WorkspaceRuntime | undefined;
  readonly origin: DaemonOrigin | undefined;
  readonly documentState: DocumentState | undefined;
  readonly documentVersion: number | undefined;
}

export function sameDaemonOrigin(
  left: DaemonOrigin | undefined,
  right: DaemonOrigin,
): boolean {
  return (
    left?.rpc === right.rpc &&
    left.generation === right.generation &&
    left.daemonInstanceId === right.daemonInstanceId
  );
}

function capturePreviewIncarnation(
  preview: PreviewState,
): Pick<PreviewState, "previewSessionId" | "origin"> {
  return {
    previewSessionId: preview.previewSessionId,
    origin: preview.origin,
  };
}

function previewOwnsIncarnation(
  runtime: WorkspaceRuntime,
  preview: PreviewState,
  expected: Pick<PreviewState, "previewSessionId" | "origin">,
): boolean {
  return (
    previewIsCurrent(runtime, preview, expected.previewSessionId) &&
    preview.origin === expected.origin
  );
}

export function previewCandidateIsCurrent(
  candidate: PreviewCandidateOrigin,
  current: PreviewCandidateCurrentState,
): boolean {
  return (
    previewCandidateIdentityIsCurrent(candidate, current) &&
    current.documentState?.version === candidate.documentVersion &&
    current.documentVersion === candidate.documentVersion
  );
}

export function previewCandidateIdentityIsCurrent(
  candidate: PreviewCandidateOrigin,
  current: PreviewCandidateCurrentState,
): boolean {
  return (
    !current.disposed &&
    !candidate.origin.rpc.closed &&
    !candidate.runtime.removed &&
    current.runtime === candidate.runtime &&
    sameDaemonOrigin(current.origin, candidate.origin) &&
    current.documentState === candidate.documentState &&
    current.documentState?.sessionId === candidate.documentSessionId
  );
}

export async function disposePreviewCandidate(
  candidate: PreviewCandidate,
): Promise<void> {
  try {
    await candidate.origin.rpc.request("fleximark/disposePreview", {
      daemonInstanceId: candidate.origin.daemonInstanceId,
      previewSessionId: candidate.result.previewSessionId,
    });
  } catch {
    // The origin connection may already be closed; candidate cleanup is best-effort.
  }
}

export interface PreviewRequestDependencies {
  currentOrigin(): DaemonOrigin | undefined;
  checkpoint(
    document: vscode.TextDocument,
    state: DocumentState,
    origin: DaemonOrigin,
  ): Promise<void>;
  identityCurrent(candidate: PreviewCandidateOrigin): boolean;
  reject(candidate: PreviewCandidate): Promise<void>;
}

export async function requestPreviewCandidate(
  runtime: WorkspaceRuntime,
  document: vscode.TextDocument,
  target: PreviewTarget,
  dependencies: PreviewRequestDependencies,
): Promise<PreviewCandidate | undefined> {
  for (let attempt = 0; attempt < 3; attempt += 1) {
    const state = runtime.documents.get(document.uri.toString());
    const origin = dependencies.currentOrigin();
    if (!state?.sessionId || !origin) return;
    const documentSessionId = state.sessionId;
    const documentVersion = document.version;
    const candidateOrigin: PreviewCandidateOrigin = {
      origin,
      runtime,
      documentUri: document.uri.toString(),
      documentState: state,
      documentSessionId,
      documentVersion,
    };
    await dependencies.checkpoint(document, state, origin);
    if (!dependencies.identityCurrent(candidateOrigin)) return;
    if (
      document.version !== documentVersion ||
      state.version !== documentVersion
    )
      continue;
    const result = await origin.rpc.request("fleximark/createPreview", {
      daemonInstanceId: origin.daemonInstanceId,
      documentSessionId,
      expectedDocumentVersion: documentVersion,
      target,
    });
    if (!result) return;
    const candidate: PreviewCandidate = { ...candidateOrigin, result };
    if (!dependencies.identityCurrent(candidate)) return candidate;
    if (
      document.version !== documentVersion ||
      state.version !== documentVersion
    ) {
      await dependencies.reject(candidate);
      continue;
    }
    return candidate;
  }
  return undefined;
}

export function previewIsCurrent(
  runtime: WorkspaceRuntime,
  preview: PreviewState,
  expectedSessionId: string,
): boolean {
  return (
    !runtime.removed &&
    preview.previewSessionId === expectedSessionId &&
    runtime.previews.get(expectedSessionId) === preview
  );
}

export function embeddedPreviewShell(
  nonce: string,
  messageToken: string,
  cspSource: string,
  scriptUri: string,
  css: string,
): string {
  return `<!doctype html><html><head><meta charset="utf-8"><meta http-equiv="Content-Security-Policy" content="default-src 'none'; script-src 'nonce-${nonce}' ${cspSource}; style-src ${cspSource} 'unsafe-inline'; font-src data:; img-src ${cspSource} data: blob:; media-src ${cspSource} blob:; frame-src https://www.youtube-nocookie.com; object-src 'none';"><meta name="viewport" content="width=device-width,initial-scale=1"><meta name="fleximark-message-token" content="${messageToken}"><style>${css}</style></head><body><main id="preview" class="markdown-body"></main><script nonce="${nonce}" src="${scriptUri}"></script></body></html>`;
}

export interface UnmatchedPreviewEventQueueLimits {
  readonly maxEventsPerPreview: number;
  readonly maxBytesPerPreview: number;
  readonly maxStreams?: number;
  readonly maxTotalEvents?: number;
  readonly maxTotalBytes?: number;
}

export interface UnmatchedPreviewEventDrain {
  readonly events: readonly PreviewEvent[];
  readonly reloadRequired: boolean;
}

export interface UnmatchedPreviewEventQueueUsage {
  readonly streams: number;
  /** Includes one entry for every reload-required marker. */
  readonly totalEvents: number;
  readonly totalBytes: number;
  readonly reloadMarkers: number;
  readonly failClosed: boolean;
}

export interface UnmatchedPreviewEventQueueOverflow {
  readonly status: "overflow";
  readonly origins: readonly DaemonOrigin[];
}

export type UnmatchedPreviewEventQueueWriteResult =
  "stored" | "ignored" | UnmatchedPreviewEventQueueOverflow;

interface PreviewEventStream {
  events: PreviewEvent[];
  bytes: number;
  reloadRequired: boolean;
}

interface OriginEventStreams {
  readonly rpc: DaemonOrigin["rpc"];
  readonly generation: number;
  readonly daemonInstanceId: string;
  readonly previews: Map<string, PreviewEventStream>;
}

const DEFAULT_UNMATCHED_PREVIEW_EVENT_LIMITS = {
  maxEventsPerPreview: 64,
  maxBytesPerPreview: 1024 * 1024,
  maxStreams: 256,
  maxTotalEvents: 1024,
  maxTotalBytes: 4 * 1024 * 1024,
} as const;

const utf8Encoder = new TextEncoder();

/** Returns the exact UTF-8 byte length of an event's JSON wire form. */
export function previewEventUtf8Bytes(event: PreviewEvent): number {
  return utf8Encoder.encode(JSON.stringify(event)).byteLength;
}

/** @deprecated Use previewEventUtf8Bytes. Kept for adapter API compatibility. */
export function conservativePreviewEventBytes(event: PreviewEvent): number {
  return previewEventUtf8Bytes(event);
}

function reloadMarkerUtf8Bytes(previewSessionId: string): number {
  return utf8Encoder.encode(
    JSON.stringify({ previewSessionId, reloadRequired: true }),
  ).byteLength;
}

/**
 * Retains events that race ahead of createPreview completion. Streams are keyed
 * by the complete connection origin and preview id, so ids reused by another
 * connection generation cannot consume one another's events.
 */
export class UnmatchedPreviewEventQueue {
  readonly #limits: Required<UnmatchedPreviewEventQueueLimits>;
  readonly #origins: OriginEventStreams[] = [];
  #streams = 0;
  #totalEvents = 0;
  #totalBytes = 0;
  #reloadMarkers = 0;
  #failClosed = false;
  #failClosedOrigins: DaemonOrigin[] = [];

  constructor(
    limits: UnmatchedPreviewEventQueueLimits = DEFAULT_UNMATCHED_PREVIEW_EVENT_LIMITS,
  ) {
    this.#limits = {
      ...DEFAULT_UNMATCHED_PREVIEW_EVENT_LIMITS,
      ...limits,
    };
    if (
      Object.values(this.#limits).some((limit) => !isPositiveInteger(limit))
    ) {
      throw new RangeError(
        "preview event queue limits must be positive integers",
      );
    }
  }

  enqueue(
    origin: DaemonOrigin,
    event: PreviewEvent,
  ): UnmatchedPreviewEventQueueWriteResult {
    if (this.#failClosed) return "ignored";
    const streams = this.#findOrCreateOrigin(origin);
    let stream = streams.previews.get(event.previewSessionId);
    if (!stream) {
      if (this.#streams >= this.#limits.maxStreams) {
        return this.#enterFailClosed(origin);
      }
      stream = { events: [], bytes: 0, reloadRequired: false };
      streams.previews.set(event.previewSessionId, stream);
      this.#streams += 1;
    }
    if (stream.reloadRequired) {
      if (event.event.type !== "full") return "ignored";
      stream.reloadRequired = false;
      this.#totalEvents -= 1;
      this.#totalBytes -= stream.bytes;
      this.#reloadMarkers -= 1;
      stream.bytes = 0;
    }

    const bytes = previewEventUtf8Bytes(event);
    if (
      bytes > this.#limits.maxBytesPerPreview ||
      stream.events.length >= this.#limits.maxEventsPerPreview ||
      stream.bytes + bytes > this.#limits.maxBytesPerPreview
    ) {
      return (
        this.#replaceWithReloadMarker(origin, event.previewSessionId, stream) ??
        "stored"
      );
    }
    if (
      this.#totalEvents >= this.#limits.maxTotalEvents ||
      this.#totalBytes + bytes > this.#limits.maxTotalBytes
    ) {
      return this.#enterFailClosed(origin);
    }
    stream.events.push(event);
    stream.bytes += bytes;
    this.#totalEvents += 1;
    this.#totalBytes += bytes;
    return "stored";
  }

  markReloadRequired(
    origin: DaemonOrigin,
    previewSessionId: string,
  ): UnmatchedPreviewEventQueueWriteResult {
    if (this.#failClosed) return "ignored";
    const streams = this.#findOrCreateOrigin(origin);
    const stream = streams.previews.get(previewSessionId);
    if (stream) {
      if (stream.reloadRequired) return "ignored";
      return (
        this.#replaceWithReloadMarker(origin, previewSessionId, stream) ??
        "stored"
      );
    }
    if (this.#streams >= this.#limits.maxStreams) {
      return this.#enterFailClosed(origin);
    }
    const markerBytes = reloadMarkerUtf8Bytes(previewSessionId);
    if (
      markerBytes > this.#limits.maxBytesPerPreview ||
      this.#totalEvents >= this.#limits.maxTotalEvents ||
      this.#totalBytes + markerBytes > this.#limits.maxTotalBytes
    ) {
      return this.#enterFailClosed(origin);
    }
    streams.previews.set(previewSessionId, {
      events: [],
      bytes: markerBytes,
      reloadRequired: true,
    });
    this.#streams += 1;
    this.#totalEvents += 1;
    this.#totalBytes += markerBytes;
    this.#reloadMarkers += 1;
    return "stored";
  }

  take(
    origin: DaemonOrigin,
    previewSessionId: string,
  ): UnmatchedPreviewEventDrain | undefined {
    return this.takeForDelivery(origin, previewSessionId);
  }

  /**
   * Reads a queue-wide fail-closed state without consuming recovery owed to
   * other previews. Only exact origin cleanup can retire the overflow state.
   */
  takeForDelivery(
    origin: DaemonOrigin,
    previewSessionId: string,
  ): UnmatchedPreviewEventDrain | undefined {
    if (this.#failClosed) return { events: [], reloadRequired: true };
    return this.#takeStream(origin, previewSessionId);
  }

  #takeStream(
    origin: DaemonOrigin,
    previewSessionId: string,
  ): UnmatchedPreviewEventDrain | undefined {
    const originIndex = this.#findOriginIndex(origin);
    if (originIndex < 0) return undefined;
    const streams = this.#origins[originIndex];
    const stream = streams.previews.get(previewSessionId);
    if (!stream) return undefined;
    this.#removeStream(stream);
    streams.previews.delete(previewSessionId);
    if (streams.previews.size === 0) this.#origins.splice(originIndex, 1);
    return {
      events: stream.events,
      reloadRequired: stream.reloadRequired,
    };
  }

  clearOrigin(origin: DaemonOrigin): void {
    // Overflow recovery belongs to the exact RPC origin that exceeded bounds.
    if (this.#failClosed) {
      const originIndex = this.#failClosedOrigins.findIndex((candidate) =>
        this.#originMatches(candidate, origin),
      );
      if (originIndex >= 0) this.#failClosedOrigins.splice(originIndex, 1);
      if (this.#failClosedOrigins.length === 0) this.clearAll();
      return;
    }
    const originIndex = this.#findOriginIndex(origin);
    if (originIndex < 0) return;
    for (const stream of this.#origins[originIndex].previews.values())
      this.#removeStream(stream);
    this.#origins.splice(originIndex, 1);
  }

  clearAll(): void {
    this.#origins.length = 0;
    this.#streams = 0;
    this.#totalEvents = 0;
    this.#totalBytes = 0;
    this.#reloadMarkers = 0;
    this.#failClosed = false;
    this.#failClosedOrigins = [];
  }

  get usage(): UnmatchedPreviewEventQueueUsage {
    return {
      streams: this.#streams,
      totalEvents: this.#totalEvents,
      totalBytes: this.#totalBytes,
      reloadMarkers: this.#reloadMarkers,
      failClosed: this.#failClosed,
    };
  }

  #findOrCreateOrigin(origin: DaemonOrigin): OriginEventStreams {
    const originIndex = this.#findOriginIndex(origin);
    if (originIndex >= 0) return this.#origins[originIndex];
    const streams: OriginEventStreams = {
      rpc: origin.rpc,
      generation: origin.generation,
      daemonInstanceId: origin.daemonInstanceId,
      previews: new Map(),
    };
    this.#origins.push(streams);
    return streams;
  }

  #findOriginIndex(origin: DaemonOrigin): number {
    return this.#origins.findIndex((candidate) =>
      this.#originMatches(candidate, origin),
    );
  }

  #replaceWithReloadMarker(
    origin: DaemonOrigin,
    previewSessionId: string,
    stream: PreviewEventStream,
  ): UnmatchedPreviewEventQueueOverflow | undefined {
    this.#totalEvents -= stream.events.length;
    this.#totalBytes -= stream.bytes;
    const markerBytes = reloadMarkerUtf8Bytes(previewSessionId);
    if (
      markerBytes > this.#limits.maxBytesPerPreview ||
      this.#totalEvents >= this.#limits.maxTotalEvents ||
      this.#totalBytes + markerBytes > this.#limits.maxTotalBytes
    ) {
      return this.#enterFailClosed(origin);
    }
    stream.events = [];
    stream.bytes = markerBytes;
    stream.reloadRequired = true;
    this.#totalEvents += 1;
    this.#totalBytes += markerBytes;
    this.#reloadMarkers += 1;
    return undefined;
  }

  #removeStream(stream: PreviewEventStream): void {
    this.#streams -= 1;
    this.#totalEvents -= stream.reloadRequired ? 1 : stream.events.length;
    this.#totalBytes -= stream.bytes;
    if (stream.reloadRequired) this.#reloadMarkers -= 1;
  }

  #enterFailClosed(origin: DaemonOrigin): UnmatchedPreviewEventQueueOverflow {
    const affectedOrigins: DaemonOrigin[] = [];
    for (const candidate of [
      ...this.#origins.map((streams) => ({
        rpc: streams.rpc,
        generation: streams.generation,
        daemonInstanceId: streams.daemonInstanceId,
      })),
      origin,
    ]) {
      if (
        !affectedOrigins.some((existing) =>
          this.#originMatches(existing, candidate),
        )
      )
        affectedOrigins.push(candidate);
    }
    this.#origins.length = 0;
    this.#streams = 0;
    // failClosed is a queue-wide state, not a retained event. `take` projects
    // it as reload-required without storing unaccounted payload bytes.
    this.#totalEvents = 0;
    this.#totalBytes = 0;
    this.#reloadMarkers = 0;
    this.#failClosed = true;
    // Keep the reported snapshot independent: closing an RPC can synchronously
    // clear its origin while a consumer is still iterating this result.
    this.#failClosedOrigins = [...affectedOrigins];
    return { status: "overflow", origins: affectedOrigins };
  }

  #originMatches(
    candidate: DaemonOrigin | undefined,
    origin: DaemonOrigin,
  ): boolean {
    return sameDaemonOrigin(candidate, origin);
  }
}

function isPositiveInteger(value: number): boolean {
  return Number.isSafeInteger(value) && value > 0;
}

export interface PreviewReadinessDependencies {
  current(runtime: WorkspaceRuntime, preview: PreviewState): boolean;
  take(
    origin: DaemonOrigin,
    previewSessionId: string,
  ): UnmatchedPreviewEventDrain | undefined;
  markReload(origin: DaemonOrigin, previewSessionId: string): void;
  deliver(origin: DaemonOrigin, event: PreviewEvent): Promise<boolean>;
  reload(runtime: WorkspaceRuntime, preview: PreviewState): Promise<boolean>;
  report(error: unknown): void;
}

function readinessIsCurrent(
  runtime: WorkspaceRuntime,
  preview: PreviewState,
  epoch: number,
  dependencies: PreviewReadinessDependencies,
): boolean {
  return (
    preview.handshakeEpoch === epoch && dependencies.current(runtime, preview)
  );
}

async function requestReadinessReload(
  runtime: WorkspaceRuntime,
  preview: PreviewState,
  epoch: number,
  origin: DaemonOrigin,
  dependencies: PreviewReadinessDependencies,
): Promise<void> {
  if (!readinessIsCurrent(runtime, preview, epoch, dependencies)) return;
  preview.reloadPending = true;
  dependencies.markReload(origin, preview.previewSessionId);
  try {
    await dependencies.reload(runtime, preview);
  } catch (error) {
    if (readinessIsCurrent(runtime, preview, epoch, dependencies))
      dependencies.report(error);
  }
}

async function drainPreviewEvents(
  runtime: WorkspaceRuntime,
  preview: PreviewState,
  epoch: number,
  dependencies: PreviewReadinessDependencies,
): Promise<boolean> {
  while (readinessIsCurrent(runtime, preview, epoch, dependencies)) {
    const origin = preview.origin;
    const drain = dependencies.take(origin, preview.previewSessionId);
    if (!drain) {
      preview.reloadPending = false;
      return true;
    }
    if (drain.reloadRequired) {
      await requestReadinessReload(
        runtime,
        preview,
        epoch,
        origin,
        dependencies,
      );
      return false;
    }
    for (const event of drain.events) {
      if (!readinessIsCurrent(runtime, preview, epoch, dependencies))
        return false;
      try {
        if (await dependencies.deliver(origin, event)) continue;
      } catch (error) {
        if (
          event.event.type === "selectSource" ||
          event.event.type === "revealSource"
        ) {
          dependencies.report(error);
          continue;
        }
      }
      await requestReadinessReload(
        runtime,
        preview,
        epoch,
        origin,
        dependencies,
      );
      return false;
    }
  }
  return false;
}

function runReadinessOperation(
  runtime: WorkspaceRuntime,
  preview: PreviewState,
  beforeDrain: (epoch: number) => Promise<boolean>,
  dependencies: PreviewReadinessDependencies,
  restartWhenReady = false,
  retryPending = false,
): Promise<void> {
  if (preview.handshakeInFlight) {
    if (retryPending) preview.handshakePending = true;
    return preview.handshakeInFlight;
  }
  if (
    (!restartWhenReady && preview.ready) ||
    !dependencies.current(runtime, preview)
  )
    return Promise.resolve();
  preview.ready = false;
  preview.handshakePending = false;
  let epoch = (preview.handshakeEpoch ?? 0) + 1;
  preview.handshakeEpoch = epoch;
  const operation = Promise.resolve().then(async () => {
    try {
      let retry = true;
      while (retry) {
        retry = false;
        if (!readinessIsCurrent(runtime, preview, epoch, dependencies)) return;
        if (!(await beforeDrain(epoch))) {
          if (!readinessIsCurrent(runtime, preview, epoch, dependencies))
            return;
        } else if (
          readinessIsCurrent(runtime, preview, epoch, dependencies) &&
          (await drainPreviewEvents(runtime, preview, epoch, dependencies))
        ) {
          if (readinessIsCurrent(runtime, preview, epoch, dependencies))
            preview.ready = true;
        }
        if (
          !retryPending ||
          !preview.handshakePending ||
          !dependencies.current(runtime, preview)
        )
          return;
        preview.handshakePending = false;
        preview.ready = false;
        epoch += 1;
        preview.handshakeEpoch = epoch;
        retry = true;
      }
    } finally {
      if (preview.handshakeInFlight === operation) {
        preview.handshakeInFlight = undefined;
        preview.handshakePending = false;
      }
    }
  });
  preview.handshakeInFlight = operation;
  return operation;
}

export function beginEmbeddedPreviewHandshake(
  runtime: WorkspaceRuntime,
  preview: PreviewState,
  dependencies: PreviewReadinessDependencies,
): Promise<void> {
  const recoveringPublication = preview.reloadPending === true;
  const initialization = {
    type: "initializePreview" as const,
    messageToken: preview.messageToken,
    publication: preview.initialPublication,
  };
  return runReadinessOperation(
    runtime,
    preview,
    async (epoch) => {
      if (recoveringPublication) return true;
      try {
        const delivered =
          await preview.panel?.webview.postMessage(initialization);
        if (delivered) return true;
      } catch (error) {
        if (readinessIsCurrent(runtime, preview, epoch, dependencies))
          dependencies.report(error);
      }
      await requestReadinessReload(
        runtime,
        preview,
        epoch,
        preview.origin,
        dependencies,
      );
      return false;
    },
    dependencies,
    true,
    true,
  );
}

export function completePreviewReadiness(
  runtime: WorkspaceRuntime,
  preview: PreviewState,
  dependencies: PreviewReadinessDependencies,
): Promise<void> {
  return runReadinessOperation(
    runtime,
    preview,
    async () => true,
    dependencies,
    false,
    true,
  );
}

export function retirePreviewHandshake(preview: PreviewState): void {
  preview.handshakeEpoch = (preview.handshakeEpoch ?? 0) + 1;
  preview.handshakeInFlight = undefined;
  preview.handshakePending = false;
  preview.reloadPending = false;
  preview.ready = false;
}

interface PreviewPublicationRecoveryDependencies {
  markReload(origin: DaemonOrigin, previewSessionId: string): void;
  reload(runtime: WorkspaceRuntime, preview: PreviewState): Promise<boolean>;
  report(error: unknown): void;
}

async function awaitAuthoritativePreviewPublication(
  runtime: WorkspaceRuntime,
  preview: PreviewState,
  dependencies: PreviewPublicationRecoveryDependencies,
): Promise<void> {
  preview.ready = false;
  preview.reloadPending = true;
  dependencies.markReload(preview.origin, preview.previewSessionId);
  try {
    await dependencies.reload(runtime, preview);
  } catch (error) {
    dependencies.report(error);
  }
}

function previewCandidateOwnsCommittedState(
  runtime: WorkspaceRuntime,
  preview: PreviewState,
  candidate: PreviewCandidate,
): boolean {
  return (
    !runtime.removed &&
    runtime.previews.get(candidate.result.previewSessionId) === preview &&
    preview.previewSessionId === candidate.result.previewSessionId &&
    preview.origin === candidate.origin
  );
}

function createPreviewState(
  candidate: PreviewCandidate,
  sourceViewColumn: vscode.ViewColumn | undefined,
  target: PreviewTarget,
): PreviewState {
  const { initialPublication, previewSessionId } = candidate.result;
  return {
    origin: candidate.origin,
    documentUri: candidate.documentUri,
    sourceViewColumn,
    previewSessionId,
    target,
    initialPublication,
    renderRevision: initialPublication.resultRenderRevision,
  };
}

export interface PreviewLifecycleDependencies {
  activeEditor(): vscode.TextEditor | undefined;
  showNoDocument(): void;
  showNoWorkspace(): void;
  workspaceFor(
    document: vscode.TextDocument,
  ): vscode.WorkspaceFolder | undefined;
  start(
    workspace: vscode.WorkspaceFolder | undefined,
  ): Promise<WorkspaceRuntime | undefined>;
  sync(document: vscode.TextDocument): Promise<void>;
  request(
    runtime: WorkspaceRuntime,
    document: vscode.TextDocument,
    target: PreviewTarget,
  ): Promise<PreviewCandidate | undefined>;
  candidateCurrent(candidate: PreviewCandidate): boolean;
  candidateIdentityCurrent(candidate: PreviewCandidate): boolean;
  rejectCandidate(candidate: PreviewCandidate): Promise<void>;
  handshake(runtime: WorkspaceRuntime, preview: PreviewState): Promise<void>;
  activate(runtime: WorkspaceRuntime, preview: PreviewState): Promise<void>;
  discardQueued(origin: DaemonOrigin, previewSessionId: string): void;
  openExternal(url: string): Promise<void>;
  createPanel(title: string): vscode.WebviewPanel;
  scriptUri(panel: vscode.WebviewPanel): string;
  random(size: number, encoding: "base64" | "base64url"): string;
  css: string;
  log(message: string): void;
  previewCurrent(runtime: WorkspaceRuntime, preview: PreviewState): boolean;
  markReload(origin: DaemonOrigin, previewSessionId: string): void;
  reload(runtime: WorkspaceRuntime, preview: PreviewState): Promise<boolean>;
  report(error: unknown): void;
  dispose(
    runtime: WorkspaceRuntime,
    preview: PreviewState,
  ): Promise<void> | void;
  notifyNavigation(event: {
    previewSessionId: string;
    renderRevision: number;
    event: EditorNavigationEvent;
  }): void;
}

export async function openPreviewLifecycle(
  target: PreviewTarget,
  dependencies: PreviewLifecycleDependencies,
): Promise<void> {
  const editor = dependencies.activeEditor();
  const document = editor?.document;
  const sourceViewColumn = editor?.viewColumn;
  if (!document || document.languageId !== "markdown") {
    dependencies.showNoDocument();
    return;
  }
  const workspace = dependencies.workspaceFor(document);
  if (!workspace) {
    dependencies.showNoWorkspace();
    return;
  }
  const runtime = await dependencies.start(workspace);
  if (!runtime) return;
  await dependencies.sync(document);
  const candidate = await dependencies.request(runtime, document, target);
  if (!candidate) return;
  if (!dependencies.candidateCurrent(candidate)) {
    await dependencies.rejectCandidate(candidate);
    return;
  }
  const result = candidate.result;
  if (target === "externalBrowser") {
    if (!result.url) {
      await dependencies.rejectCandidate(candidate);
      throw new Error("daemon omitted the external preview URL");
    }
    const preview = createPreviewState(candidate, sourceViewColumn, target);
    preview.ready = false;
    runtime.previews.set(result.previewSessionId, preview);
    const candidateOwnsState = () =>
      dependencies.candidateIdentityCurrent(candidate) &&
      previewCandidateOwnsCommittedState(runtime, preview, candidate);
    try {
      await dependencies.openExternal(result.url);
    } catch (error) {
      dependencies.discardQueued(candidate.origin, result.previewSessionId);
      if (candidateOwnsState()) {
        preview.ready = true;
        throw error;
      }
      return;
    }
    if (!candidateOwnsState()) return;
    if (!dependencies.candidateCurrent(candidate)) {
      await awaitAuthoritativePreviewPublication(
        runtime,
        preview,
        dependencies,
      );
      return;
    }
    await dependencies.activate(runtime, preview);
    return;
  }

  let panel: vscode.WebviewPanel | undefined;
  let preview: PreviewState | undefined;
  try {
    panel = dependencies.createPanel(
      `FlexiMark: ${path.basename(document.fileName)}`,
    );
    const nonce = dependencies.random(16, "base64");
    const messageToken = dependencies.random(32, "base64url");
    const shell = embeddedPreviewShell(
      nonce,
      messageToken,
      panel.webview.cspSource,
      dependencies.scriptUri(panel),
      dependencies.css,
    );
    if (!dependencies.candidateCurrent(candidate)) {
      panel.dispose();
      panel = undefined;
      await dependencies.rejectCandidate(candidate);
      return;
    }
    preview = createPreviewState(candidate, sourceViewColumn, target);
    preview.messageToken = messageToken;
    preview.panel = panel;
    runtime.previews.set(result.previewSessionId, preview);
    dependencies.log(
      `embedded preview created revision=${preview.renderRevision}`,
    );
    panel.webview.onDidReceiveMessage((event: unknown) => {
      if (!isWebviewInboundMessage(event)) return;
      if (!preview || !dependencies.previewCurrent(runtime, preview)) return;
      if (event.type === "ready") {
        dependencies.log("embedded preview webview ready");
        const readyPreview = preview;
        const incarnation = capturePreviewIncarnation(readyPreview);
        const handshake = dependencies.handshake(runtime, readyPreview);
        const handshakeEpoch = readyPreview.handshakeEpoch;
        void handshake.catch((error: unknown) => {
          if (
            previewOwnsIncarnation(runtime, readyPreview, incarnation) &&
            readyPreview.handshakeEpoch === handshakeEpoch &&
            dependencies.previewCurrent(runtime, readyPreview)
          )
            dependencies.report(error);
        });
        return;
      }
      if (
        event.type === "rendered" &&
        event.previewSessionId === preview.previewSessionId &&
        event.renderRevision === preview.renderRevision
      ) {
        preview.renderedRevision = event.renderRevision;
        dependencies.log(
          `embedded preview rendered revision=${event.renderRevision}`,
        );
        return;
      }
      if (event.type === "requestSnapshot") {
        void dependencies.reload(runtime, preview);
        return;
      }
      if (
        shouldForwardEditorNavigation(
          event,
          preview.previewSessionId,
          preview.renderRevision,
        )
      )
        dependencies.notifyNavigation({
          previewSessionId: event.previewSessionId,
          renderRevision: event.renderRevision,
          event,
        });
    });
    panel.onDidDispose(() => {
      if (preview) dependencies.dispose(runtime, preview);
    });
    panel.webview.html = shell;
  } catch (error) {
    if (preview && runtime.previews.get(result.previewSessionId) === preview)
      runtime.previews.delete(result.previewSessionId);
    panel?.dispose();
    await dependencies.rejectCandidate(candidate);
    throw error;
  }
}

export interface PreviewDisposalDependencies {
  clearQueued(origin: DaemonOrigin, previewSessionId: string): void;
  reportFailure(message: string): void;
}

export async function disposePreviewLifecycle(
  runtime: WorkspaceRuntime,
  preview: PreviewState,
  dependencies: PreviewDisposalDependencies,
): Promise<void> {
  const previewSessionId = preview.previewSessionId;
  if (
    runtime.previews.get(previewSessionId) !== preview ||
    !runtime.previews.delete(previewSessionId)
  )
    return;
  dependencies.clearQueued(preview.origin, previewSessionId);
  try {
    await preview.origin.rpc.request("fleximark/disposePreview", {
      daemonInstanceId: preview.origin.daemonInstanceId,
      previewSessionId,
    });
  } catch (error) {
    dependencies.reportFailure(`preview disposal failed: ${String(error)}`);
  }
  preview.panel?.dispose();
}

export interface PreviewReloadDependencies {
  disposed(): boolean;
  currentOrigin(): DaemonOrigin | undefined;
  reportFailure(message: string): void;
}

export async function reloadPreviewLifecycle(
  runtime: WorkspaceRuntime,
  preview: PreviewState,
  dependencies: PreviewReloadDependencies,
): Promise<boolean> {
  const previewSessionId = preview.previewSessionId;
  const incarnation = capturePreviewIncarnation(preview);
  const expectedIncarnationIsRegistered = () =>
    previewOwnsIncarnation(runtime, preview, incarnation);
  try {
    const origin = dependencies.currentOrigin();
    if (
      dependencies.disposed() ||
      !previewIsCurrent(runtime, preview, previewSessionId) ||
      !sameDaemonOrigin(origin, incarnation.origin)
    )
      return false;
    await incarnation.origin.rpc.request("fleximark/reloadPreview", {
      daemonInstanceId: incarnation.origin.daemonInstanceId,
      previewSessionId,
    });
    return expectedIncarnationIsRegistered();
  } catch (error) {
    if (expectedIncarnationIsRegistered())
      dependencies.reportFailure(`preview reload failed: ${String(error)}`);
    return false;
  }
}

export interface PreviewRecreationDependencies {
  document(uri: string): vscode.TextDocument | undefined;
  request(
    runtime: WorkspaceRuntime,
    document: vscode.TextDocument,
    target: PreviewTarget,
  ): Promise<PreviewCandidate | undefined>;
  currentOrigin(): DaemonOrigin | undefined;
  candidateCurrent(candidate: PreviewCandidate): boolean;
  candidateIdentityCurrent(candidate: PreviewCandidate): boolean;
  rejectCandidate(candidate: PreviewCandidate): Promise<void>;
  dispose(runtime: WorkspaceRuntime, preview: PreviewState): Promise<void>;
  handshake(runtime: WorkspaceRuntime, preview: PreviewState): Promise<void>;
  activate(runtime: WorkspaceRuntime, preview: PreviewState): Promise<void>;
  discardQueued(origin: DaemonOrigin, previewSessionId: string): void;
  markReload(origin: DaemonOrigin, previewSessionId: string): void;
  reload(runtime: WorkspaceRuntime, preview: PreviewState): Promise<boolean>;
  openExternal(url: string): Promise<void>;
  report(error: unknown): void;
}

export async function recreatePreviewsLifecycle(
  runtime: WorkspaceRuntime,
  dependencies: PreviewRecreationDependencies,
): Promise<void> {
  for (const [previousId, preview] of [...runtime.previews]) {
    let committedCandidate = false;
    let committedEmbeddedCandidate: PreviewCandidate | undefined;
    let committedExternalCandidate: PreviewCandidate | undefined;
    const candidateOwnsState = (candidate: PreviewCandidate | undefined) =>
      candidate !== undefined &&
      dependencies.candidateIdentityCurrent(candidate) &&
      previewCandidateOwnsCommittedState(runtime, preview, candidate);
    const disposeStaleOldPreview = async () => {
      const replacementOrigin = dependencies.currentOrigin();
      if (
        replacementOrigin &&
        !replacementOrigin.rpc.closed &&
        !sameDaemonOrigin(replacementOrigin, preview.origin) &&
        runtime.previews.get(previousId) === preview
      )
        await dependencies.dispose(runtime, preview);
    };
    const document = dependencies.document(preview.documentUri);
    if (!document) {
      void dependencies.dispose(runtime, preview);
      continue;
    }
    try {
      const candidate = await dependencies.request(
        runtime,
        document,
        preview.target,
      );
      if (!candidate) {
        await disposeStaleOldPreview();
        continue;
      }
      if (!preview.panel && !candidate.result.url) {
        await dependencies.rejectCandidate(candidate);
        throw new Error("daemon omitted the external preview URL");
      }
      if (!dependencies.candidateCurrent(candidate)) {
        await dependencies.rejectCandidate(candidate);
        await disposeStaleOldPreview();
        continue;
      }
      if (runtime.previews.get(previousId) !== preview) {
        await dependencies.rejectCandidate(candidate);
        continue;
      }
      retirePreviewHandshake(preview);
      if (!runtime.previews.delete(previousId)) {
        await dependencies.rejectCandidate(candidate);
        continue;
      }
      const result = candidate.result;
      preview.previewSessionId = result.previewSessionId;
      preview.origin = candidate.origin;
      preview.initialPublication = result.initialPublication;
      preview.renderRevision = result.initialPublication.resultRenderRevision;
      preview.renderedRevision = undefined;
      preview.ready = false;
      runtime.previews.set(result.previewSessionId, preview);
      committedCandidate = true;
      if (preview.panel) {
        committedEmbeddedCandidate = candidate;
        try {
          await dependencies.handshake(runtime, preview);
        } catch (error) {
          if (candidateOwnsState(candidate)) {
            await dependencies.reload(runtime, preview);
            if (candidateOwnsState(candidate)) dependencies.report(error);
          }
          continue;
        }
        if (!candidateOwnsState(candidate)) continue;
        if (!preview.ready) continue;
        if (!dependencies.candidateCurrent(candidate)) {
          await awaitAuthoritativePreviewPublication(
            runtime,
            preview,
            dependencies,
          );
          continue;
        }
        await dependencies.activate(runtime, preview);
      } else {
        if (!result.url) continue;
        committedExternalCandidate = candidate;
        await dependencies.openExternal(result.url);
        if (!candidateOwnsState(candidate)) continue;
        if (!dependencies.candidateCurrent(candidate))
          await awaitAuthoritativePreviewPublication(
            runtime,
            preview,
            dependencies,
          );
        else await dependencies.activate(runtime, preview);
      }
    } catch (error) {
      const embeddedCandidateOwnsState = candidateOwnsState(
        committedEmbeddedCandidate,
      );
      const externalCandidateOwnsState = candidateOwnsState(
        committedExternalCandidate,
      );
      if (committedExternalCandidate) {
        dependencies.discardQueued(
          committedExternalCandidate.origin,
          committedExternalCandidate.result.previewSessionId,
        );
        if (externalCandidateOwnsState) preview.ready = true;
      }
      if (embeddedCandidateOwnsState)
        await dependencies.dispose(runtime, preview);
      else if (!committedCandidate) await disposeStaleOldPreview();
      if (
        committedEmbeddedCandidate
          ? embeddedCandidateOwnsState
          : committedExternalCandidate
            ? externalCandidateOwnsState
            : true
      )
        dependencies.report(error);
    }
  }
}

export interface PreviewEventDependencies {
  reload(runtime: WorkspaceRuntime, preview: PreviewState): void;
  overflow?(origin: DaemonOrigin): void;
  navigate(
    runtime: WorkspaceRuntime,
    preview: PreviewState,
    event: SourceNavigationEvent,
  ): Promise<void>;
  reactivate?(runtime: WorkspaceRuntime, preview: PreviewState): Promise<void>;
  report?(error: unknown): void;
}

export async function handlePreviewEventLifecycle(
  origin: DaemonOrigin,
  event: PreviewEvent,
  runtimes: Iterable<WorkspaceRuntime>,
  queue: UnmatchedPreviewEventQueue,
  dependencies: PreviewEventDependencies,
  fromQueue = false,
): Promise<boolean> {
  const runtime = [...runtimes].find((candidate) =>
    candidate.previews.has(event.previewSessionId),
  );
  const preview = runtime?.previews.get(event.previewSessionId);
  if (
    !runtime ||
    !preview ||
    !sameDaemonOrigin(preview.origin, origin) ||
    (!preview.ready && !fromQueue)
  ) {
    const queued = queue.enqueue(origin, event);
    if (typeof queued === "object") {
      for (const affectedOrigin of queued.origins)
        dependencies.overflow?.(affectedOrigin);
    } else if (
      runtime &&
      preview?.reloadPending &&
      event.event.type === "full" &&
      dependencies.reactivate
    )
      void dependencies
        .reactivate(runtime, preview)
        .catch((error: unknown) => dependencies.report?.(error));
    return true;
  }
  const action = previewEventAction(preview.renderRevision, event);
  if (action === "ignore") return true;
  if (action === "reload") {
    if (!fromQueue) dependencies.reload(runtime, preview);
    return false;
  }
  if (
    event.event.type === "selectSource" ||
    event.event.type === "revealSource"
  ) {
    await dependencies.navigate(runtime, preview, event.event);
    return true;
  }
  if (event.event.type === "full" || event.event.type === "patch")
    preview.renderRevision = event.event.resultRenderRevision;
  if (!preview.panel) return true;
  const incarnation = capturePreviewIncarnation(preview);
  try {
    const delivered = await preview.panel.webview.postMessage({
      type: "previewEvent",
      messageToken: preview.messageToken,
      event: event.event,
    });
    if (
      !delivered &&
      !fromQueue &&
      previewOwnsIncarnation(runtime, preview, incarnation)
    )
      dependencies.reload(runtime, preview);
    return delivered;
  } catch {
    if (!fromQueue && previewOwnsIncarnation(runtime, preview, incarnation))
      dependencies.reload(runtime, preview);
    return false;
  }
}

export interface PreviewNavigationDependencies {
  document(uri: string): vscode.TextDocument | undefined;
  sourcePositionInDocument(
    document: vscode.TextDocument,
    position: SourceNavigationEvent["sourceRange"]["start"],
  ): boolean;
  range(
    document: vscode.TextDocument,
    event: SourceNavigationEvent,
  ): vscode.Range;
  visibleEditor(
    documentUri: string,
    sourceViewColumn: vscode.ViewColumn | undefined,
  ): vscode.TextEditor | undefined;
  showEditor(
    document: vscode.TextDocument,
    sourceViewColumn: vscode.ViewColumn | undefined,
  ): PromiseLike<vscode.TextEditor>;
  current(runtime: WorkspaceRuntime, preview: PreviewState): boolean;
  documentOpen(document: vscode.TextDocument): boolean;
  selection(range: vscode.Range): vscode.Selection;
  select(editor: vscode.TextEditor, selection: vscode.Selection): void;
  reveal(
    editor: vscode.TextEditor,
    range: vscode.Range,
    kind: "center" | "top",
  ): void;
  setSelectionEcho(uri: string, value: string): object;
  clearSelectionEcho(token: object): void;
  setViewportEcho(uri: string): object;
  clearViewportEcho(token: object): void;
  schedule(callback: () => void, delay: number): void;
}

export interface PreviewEchoState {
  selection?: { uri: string; value: string };
  viewport?: { uri: string };
}

export interface EditorPreviewEventDependencies {
  runtime(document: vscode.TextDocument): WorkspaceRuntime | undefined;
  currentOrigin(): DaemonOrigin | undefined;
}

export function handleEditorSelectionLifecycle(
  event: vscode.TextEditorSelectionChangeEvent,
  echoes: PreviewEchoState,
  dependencies: EditorPreviewEventDependencies,
): void {
  const uri = event.textEditor.document.uri.toString();
  const selectionValue = event.selections
    .map(
      ({ anchor, active }) =>
        `${anchor.line}:${anchor.character}-${active.line}:${active.character}`,
    )
    .join(",");
  if (
    echoes.selection?.uri === uri &&
    echoes.selection.value === selectionValue
  ) {
    echoes.selection = undefined;
    return;
  }
  const runtime = dependencies.runtime(event.textEditor.document);
  const state = runtime?.documents.get(uri);
  const origin = dependencies.currentOrigin();
  if (!state?.sessionId || !origin) return;
  origin.rpc.notify("fleximark/setSelection", {
    daemonInstanceId: origin.daemonInstanceId,
    documentSessionId: state.sessionId,
    expectedDocumentVersion: event.textEditor.document.version,
    selections: event.selections.map((selection) => ({
      anchor: selection.anchor,
      active: selection.active,
    })),
  });
}

export function handleEditorViewportLifecycle(
  event: vscode.TextEditorVisibleRangesChangeEvent,
  echoes: PreviewEchoState,
  dependencies: EditorPreviewEventDependencies,
): void {
  const uri = event.textEditor.document.uri.toString();
  if (echoes.viewport?.uri === uri) {
    echoes.viewport = undefined;
    return;
  }
  const runtime = dependencies.runtime(event.textEditor.document);
  const state = runtime?.documents.get(uri);
  const origin = dependencies.currentOrigin();
  if (!state?.sessionId || !origin) return;
  origin.rpc.notify("fleximark/setViewport", {
    daemonInstanceId: origin.daemonInstanceId,
    documentSessionId: state.sessionId,
    expectedDocumentVersion: event.textEditor.document.version,
    ranges: event.visibleRanges.map((range) => ({
      start: { line: range.start.line, character: range.start.character },
      end: { line: range.end.line, character: range.end.character },
    })),
  });
}

export async function applySourceNavigationLifecycle(
  runtime: WorkspaceRuntime,
  preview: PreviewState,
  event: SourceNavigationEvent,
  dependencies: PreviewNavigationDependencies,
): Promise<void> {
  const expectedRevision = preview.renderRevision;
  const incarnation = capturePreviewIncarnation(preview);
  const document = dependencies.document(preview.documentUri);
  if (!document) return;
  const expectedDocumentVersion = document.version;
  const { start, end } = event.sourceRange;
  if (
    !dependencies.sourcePositionInDocument(document, start) ||
    !dependencies.sourcePositionInDocument(document, end)
  )
    return;
  const range = dependencies.range(document, event);
  const editor =
    dependencies.visibleEditor(preview.documentUri, preview.sourceViewColumn) ??
    (await dependencies.showEditor(document, preview.sourceViewColumn));
  if (
    !dependencies.current(runtime, preview) ||
    !previewOwnsIncarnation(runtime, preview, incarnation) ||
    preview.renderRevision !== expectedRevision ||
    !dependencies.documentOpen(document) ||
    document.version !== expectedDocumentVersion ||
    editor.document !== document
  )
    return;
  if (event.type === "selectSource") {
    const selection = dependencies.selection(range);
    const selectionEcho = dependencies.setSelectionEcho(
      preview.documentUri,
      `${selection.anchor.line}:${selection.anchor.character}-${selection.active.line}:${selection.active.character}`,
    );
    dependencies.select(editor, selection);
    dependencies.schedule(
      () => dependencies.clearSelectionEcho(selectionEcho),
      500,
    );
    const viewportEcho = dependencies.setViewportEcho(preview.documentUri);
    dependencies.reveal(editor, range, "center");
    dependencies.schedule(
      () => dependencies.clearViewportEcho(viewportEcho),
      500,
    );
    return;
  }
  const viewportEcho = dependencies.setViewportEcho(preview.documentUri);
  dependencies.reveal(editor, range, "top");
  dependencies.schedule(
    () => dependencies.clearViewportEcho(viewportEcho),
    500,
  );
}
