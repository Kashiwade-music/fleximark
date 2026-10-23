import * as path from "node:path";
import type * as vscode from "vscode";

import {
  type CreatePreviewResult,
  type EditorNavigationEvent,
  type PreviewChangedParams,
  type PreviewTarget,
  type ServerPreviewEventParams,
  type SourceNavigationEvent,
  shouldForwardEditorNavigation,
} from "../../../contracts/typescript/protocol.mjs";
import { isWebviewInboundMessage } from "../webview/bridge.mjs";
import type { DaemonOrigin } from "./document-coordinator.mjs";
import type {
  DocumentRuntime,
  DocumentState,
  PreviewState,
} from "./runtime-state.mjs";

export interface PreviewCandidateOrigin {
  readonly origin: DaemonOrigin;
  readonly runtime: DocumentRuntime;
  readonly documentUri: string;
  readonly documentState: DocumentState;
  readonly documentSessionId: string;
  readonly documentVersion: number;
}

export interface PreviewCandidate extends PreviewCandidateOrigin {
  readonly result: CreatePreviewResult;
  readonly preview?: PreviewState;
}

export interface PreviewCandidateCurrentState {
  readonly disposed: boolean;
  readonly runtime: DocumentRuntime | undefined;
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

function previewOwnsIncarnation(
  runtime: DocumentRuntime,
  preview: PreviewState,
  expected: PreviewState["binding"],
): boolean {
  return (
    preview.binding === expected &&
    previewIsCurrent(runtime, preview, expected.previewSessionId)
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
  if (
    candidate.preview &&
    candidate.runtime.previews.get(candidate.result.previewSessionId) ===
      candidate.preview
  )
    candidate.runtime.previews.delete(candidate.result.previewSessionId);
  try {
    await candidate.origin.rpc.request("fleximark/disposePreview", {
      daemonInstanceId: candidate.origin.daemonInstanceId,
      previewSessionId: candidate.result.previewSessionId,
    });
  } catch {
    // Candidate cleanup is best-effort after a connection is replaced.
  }
}

export async function requestPreviewCandidate(
  runtime: DocumentRuntime,
  document: vscode.TextDocument,
  target: PreviewTarget,
  currentOrigin: () => DaemonOrigin | undefined,
  identityCurrent: (candidate: PreviewCandidateOrigin) => boolean,
  reject: (candidate: PreviewCandidate) => Promise<void>,
): Promise<PreviewCandidate | undefined> {
  for (let attempt = 0; attempt < 3; attempt += 1) {
    const state = runtime.documents.get(document.uri.toString());
    const origin = currentOrigin();
    if (!state?.sessionId || !origin) return;
    const documentVersion = document.version;
    const candidateOrigin: PreviewCandidateOrigin = {
      origin,
      runtime,
      documentUri: document.uri.toString(),
      documentState: state,
      documentSessionId: state.sessionId,
      documentVersion,
    };
    if (!identityCurrent(candidateOrigin)) return;
    if (
      document.version !== documentVersion ||
      state.version !== documentVersion
    )
      continue;
    let preview: PreviewState | undefined;
    const result = await origin.rpc.request(
      "fleximark/createPreview",
      {
        daemonInstanceId: origin.daemonInstanceId,
        documentSessionId: state.sessionId,
        expectedDocumentVersion: documentVersion,
        target,
      },
      undefined,
      (result) => {
        preview = {
          documentUri: candidateOrigin.documentUri,
          target,
          binding: {
            origin,
            previewSessionId: result.previewSessionId,
            active: true,
            renderRevision: 0,
            frame: result.frame,
          },
        };
        runtime.previews.set(result.previewSessionId, preview);
      },
    );
    if (!result) return;
    const candidate: PreviewCandidate = { ...candidateOrigin, result, preview };
    if (!identityCurrent(candidate)) return candidate;
    if (
      document.version !== documentVersion ||
      state.version !== documentVersion
    ) {
      await reject(candidate);
      continue;
    }
    return candidate;
  }
  return undefined;
}

export function previewIsCurrent(
  runtime: DocumentRuntime,
  preview: PreviewState,
  expectedSessionId: string,
): boolean {
  return (
    !runtime.removed &&
    preview.binding.active &&
    preview.binding.previewSessionId === expectedSessionId &&
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
  return `<!doctype html><html><head><meta charset="utf-8"><meta http-equiv="Content-Security-Policy" content="default-src 'none'; base-uri 'none'; form-action 'none'; script-src 'nonce-${nonce}' ${cspSource}; style-src ${cspSource} 'unsafe-inline'; font-src data:; img-src ${cspSource} https://i.ytimg.com data: blob:; media-src ${cspSource} blob:; frame-src https://www.youtube-nocookie.com; object-src 'none';"><meta name="referrer" content="strict-origin-when-cross-origin"><meta name="viewport" content="width=device-width,initial-scale=1"><meta name="fleximark-message-token" content="${messageToken}"><style>${css}</style></head><body><main id="preview" class="markdown-body"></main><script nonce="${nonce}" src="${scriptUri}"></script></body></html>`;
}

/** Requests a fresh render while discarding completion from a replaced preview. */
export async function rerenderPreviewLifecycle(
  runtime: DocumentRuntime,
  preview: PreviewState,
  dependencies: PreviewFramePort,
): Promise<boolean> {
  if (!dependencies.previewCurrent(runtime, preview)) return false;
  const incarnation = preview.binding;
  try {
    await incarnation.origin.rpc.request("fleximark/rerenderPreview", {
      daemonInstanceId: incarnation.origin.daemonInstanceId,
      previewSessionId: incarnation.previewSessionId,
    });
  } catch (error) {
    if (
      previewOwnsIncarnation(runtime, preview, incarnation) &&
      dependencies.previewCurrent(runtime, preview)
    )
      throw error;
    return false;
  }
  return (
    previewOwnsIncarnation(runtime, preview, incarnation) &&
    dependencies.previewCurrent(runtime, preview)
  );
}

/** Delivers the latest complete frame; connection identity fences async completion. */
export async function synchronizePreviewLifecycle(
  runtime: DocumentRuntime,
  preview: PreviewState,
  dependencies: PreviewFramePort,
  force = false,
): Promise<void> {
  if (
    !preview.panel ||
    !preview.webviewReady ||
    !dependencies.previewCurrent(runtime, preview)
  )
    return;
  const binding = preview.binding;
  const frame = binding.frame;
  if (!force && frame.renderRevision <= binding.renderRevision) return;
  try {
    const delivered = await preview.panel.webview.postMessage({
      type: "previewFrame",
      messageToken: preview.messageToken,
      frame,
    });
    if (delivered && previewOwnsIncarnation(runtime, preview, binding)) {
      binding.renderRevision = Math.max(
        binding.renderRevision,
        frame.renderRevision,
      );
      const navigation = binding.pendingNavigation;
      if (navigation && navigation.renderRevision <= binding.renderRevision) {
        binding.pendingNavigation = undefined;
        if (navigation.renderRevision === binding.renderRevision)
          await preview.panel.webview.postMessage({
            type: "previewEvent",
            messageToken: preview.messageToken,
            event: navigation,
          });
      }
    }
  } catch (error) {
    if (previewOwnsIncarnation(runtime, preview, binding))
      dependencies.report(error);
  }
}

function candidateBinding(
  candidate: PreviewCandidate,
): PreviewState["binding"] {
  return (
    candidate.preview?.binding ?? {
      origin: candidate.origin,
      previewSessionId: candidate.result.previewSessionId,
      active: true,
      renderRevision: 0,
      frame: candidate.result.frame,
    }
  );
}

function createPreviewState(
  candidate: PreviewCandidate,
  sourceViewColumn: vscode.ViewColumn | undefined,
  target: PreviewTarget,
): PreviewState {
  const preview = candidate.preview ?? {
    binding: candidateBinding(candidate),
    documentUri: candidate.documentUri,
    target,
  };
  preview.sourceViewColumn = sourceViewColumn;
  return preview;
}

export interface PreviewLifecycleDependencies {
  activeEditor(): vscode.TextEditor | undefined;
  document(uri: string): vscode.TextDocument | undefined;
  showNoDocument(): void;
  workspaceFor(
    document: vscode.TextDocument,
  ): vscode.WorkspaceFolder | undefined;
  start(
    workspace: vscode.WorkspaceFolder | undefined,
  ): Promise<DocumentRuntime | undefined>;
  sync(document: vscode.TextDocument): Promise<void>;
  request(
    runtime: DocumentRuntime,
    document: vscode.TextDocument,
    target: PreviewTarget,
  ): Promise<PreviewCandidate | undefined>;
  candidateCurrent(candidate: PreviewCandidate): boolean;
  rejectCandidate(candidate: PreviewCandidate): Promise<void>;
  synchronize(
    runtime: DocumentRuntime,
    preview: PreviewState,
    force?: boolean,
  ): Promise<void>;
  openExternal(url: string): Promise<void>;
  createPanel(title: string): vscode.WebviewPanel;
  scriptUri(panel: vscode.WebviewPanel): string;
  random(size: number, encoding: "base64" | "base64url"): string;
  css: string;
  log(message: string): void;
  previewCurrent(runtime: DocumentRuntime, preview: PreviewState): boolean;
  owner(preview: PreviewState): DocumentRuntime | undefined;
  report(error: unknown): void;
  dispose(
    runtime: DocumentRuntime,
    preview: PreviewState,
  ): Promise<void> | void;
  notifyNavigation(event: {
    previewSessionId: string;
    renderRevision: number;
    event: EditorNavigationEvent;
  }): void;
}

type PreviewFramePort = Pick<
  PreviewLifecycleDependencies,
  "previewCurrent" | "report"
>;

export async function openPreviewLifecycle(
  target: PreviewTarget,
  dependencies: PreviewLifecycleDependencies,
): Promise<void> {
  const editor = dependencies.activeEditor();
  const document = editor?.document;
  if (!document || document.languageId !== "markdown") {
    dependencies.showNoDocument();
    return;
  }
  const workspace = dependencies.workspaceFor(document);
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
    const preview = createPreviewState(candidate, editor?.viewColumn, target);
    runtime.previews.set(result.previewSessionId, preview);
    const candidateOwnsState = () =>
      runtime.previews.get(result.previewSessionId) === preview &&
      preview.binding.origin === candidate.origin;
    try {
      await dependencies.openExternal(result.url);
    } catch (error) {
      if (!candidateOwnsState()) return;
      await dependencies.dispose(runtime, preview);
      throw error;
    }
    if (!candidateOwnsState()) return;
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
    preview = createPreviewState(candidate, editor?.viewColumn, target);
    preview.messageToken = messageToken;
    preview.panel = panel;
    runtime.previews.set(result.previewSessionId, preview);
    dependencies.log("embedded preview created");
    panel.webview.onDidReceiveMessage((event: unknown) => {
      if (!isWebviewInboundMessage(event) || !preview) return;
      const owner = dependencies.owner(preview);
      if (!owner || !dependencies.previewCurrent(owner, preview)) return;
      if (event.type === "ready") {
        preview.webviewReady = true;
        void dependencies.synchronize(owner, preview, true);
        return;
      }
      if (event.type === "requestFrame") {
        void dependencies.synchronize(owner, preview, true);
        return;
      }
      if (
        shouldForwardEditorNavigation(
          event,
          preview.binding.previewSessionId,
          preview.binding.renderRevision,
        )
      )
        dependencies.notifyNavigation({
          previewSessionId: event.previewSessionId,
          renderRevision: event.renderRevision,
          event,
        });
    });
    panel.onDidDispose(() => {
      if (!preview) return;
      const owner = dependencies.owner(preview);
      if (owner) dependencies.dispose(owner, preview);
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

export async function followActiveDocumentLifecycle(
  editor: vscode.TextEditor | undefined,
  runtimes: Iterable<DocumentRuntime>,
  dependencies: PreviewLifecycleDependencies,
): Promise<void> {
  const document = editor?.document;
  const sourceViewColumn = editor?.viewColumn;
  if (
    !document ||
    document.languageId !== "markdown" ||
    sourceViewColumn === undefined
  )
    return;
  const workspace = dependencies.workspaceFor(document);
  const owners = [...runtimes];
  const followers = owners.flatMap((runtime) =>
    [...runtime.previews.values()]
      .filter(
        (preview) =>
          preview.panel &&
          preview.target === "embeddedHtml" &&
          preview.sourceViewColumn === sourceViewColumn &&
          (preview.documentUri !== document.uri.toString() ||
            !preview.binding.active),
      )
      .map((preview) => ({ runtime, preview })),
  );
  if (followers.length === 0) return;
  const targetRuntime = await dependencies.start(workspace);
  if (!targetRuntime) return;
  await dependencies.sync(document);

  await Promise.all(
    followers.map(async ({ runtime: previousRuntime, preview }) => {
      const previousId = preview.binding.previewSessionId;
      const previousOrigin = preview.binding.origin;
      const candidate = await dependencies.request(
        targetRuntime,
        document,
        "embeddedHtml",
      );
      if (!candidate) return;
      const activeEditor = dependencies.activeEditor();
      const stillActive =
        activeEditor?.document.uri.toString() === document.uri.toString() &&
        activeEditor.viewColumn === sourceViewColumn;
      if (
        !stillActive ||
        !dependencies.candidateCurrent(candidate) ||
        previousRuntime.previews.get(previousId) !== preview
      ) {
        await dependencies.rejectCandidate(candidate);
        return;
      }

      const previousRemoteSessionActive = preview.binding.active;
      previousRuntime.previews.delete(previousId);
      preview.binding = candidateBinding(candidate);
      preview.documentUri = candidate.documentUri;
      preview.sourceViewColumn = sourceViewColumn;
      targetRuntime.previews.set(preview.binding.previewSessionId, preview);
      if (preview.panel)
        preview.panel.title = `FlexiMark: ${path.basename(document.fileName)}`;
      if (previousRemoteSessionActive)
        try {
          await previousOrigin.rpc.request("fleximark/disposePreview", {
            daemonInstanceId: previousOrigin.daemonInstanceId,
            previewSessionId: previousId,
          });
        } catch {
          // The old connection may already have been replaced.
        }
      if (dependencies.previewCurrent(targetRuntime, preview))
        await dependencies.synchronize(targetRuntime, preview, true);
    }),
  );
}

export async function disposePreviewLifecycle(
  runtime: DocumentRuntime,
  preview: PreviewState,
  reportFailure: (message: string) => void,
): Promise<void> {
  const previewSessionId = preview.binding.previewSessionId;
  if (
    runtime.previews.get(previewSessionId) !== preview ||
    !runtime.previews.delete(previewSessionId)
  )
    return;
  if (preview.binding.active)
    try {
      await preview.binding.origin.rpc.request("fleximark/disposePreview", {
        daemonInstanceId: preview.binding.origin.daemonInstanceId,
        previewSessionId,
      });
    } catch (error) {
      reportFailure(`preview disposal failed: ${String(error)}`);
    }
  preview.panel?.dispose();
}

export async function recreatePreviewsLifecycle(
  runtime: DocumentRuntime,
  dependencies: PreviewLifecycleDependencies,
): Promise<void> {
  for (const [previousId, preview] of [...runtime.previews]) {
    const document = dependencies.document(preview.documentUri);
    if (!document) {
      preview.binding.active = false;
      if (!preview.panel) await dependencies.dispose(runtime, preview);
      continue;
    }
    try {
      const candidate = await dependencies.request(
        runtime,
        document,
        preview.target,
      );
      if (!candidate) continue;
      if (
        !dependencies.candidateCurrent(candidate) ||
        runtime.previews.get(previousId) !== preview
      ) {
        await dependencies.rejectCandidate(candidate);
        continue;
      }
      if (!preview.panel && !candidate.result.url) {
        await dependencies.rejectCandidate(candidate);
        throw new Error("daemon omitted the external preview URL");
      }
      runtime.previews.delete(previousId);
      preview.binding = candidateBinding(candidate);
      runtime.previews.set(preview.binding.previewSessionId, preview);
      if (preview.panel) await dependencies.synchronize(runtime, preview, true);
      else if (candidate.result.url)
        await dependencies.openExternal(candidate.result.url);
    } catch (error) {
      dependencies.report(error);
    }
  }
}

export async function handlePreviewEventLifecycle(
  origin: DaemonOrigin,
  event: ServerPreviewEventParams,
  runtimes: Iterable<DocumentRuntime>,
  navigate: (
    runtime: DocumentRuntime,
    preview: PreviewState,
    event: SourceNavigationEvent,
  ) => Promise<void>,
): Promise<boolean> {
  const runtime = [...runtimes].find((candidate) =>
    candidate.previews.has(event.previewSessionId),
  );
  const preview = runtime?.previews.get(event.previewSessionId);
  if (
    !runtime ||
    !preview ||
    !preview.binding.active ||
    !sameDaemonOrigin(preview.binding.origin, origin)
  )
    return false;
  // Browser SSE delivery is independent of the adapter. The daemon validates
  // source navigation against its current frame before forwarding it here.
  if (!preview.panel && preview.target === "externalBrowser") {
    if (
      event.renderRevision <
      Math.max(
        preview.binding.renderRevision,
        preview.binding.frame.renderRevision,
      )
    )
      return true;
    preview.binding.renderRevision = event.renderRevision;
  } else if (event.renderRevision !== preview.binding.renderRevision) {
    if (
      event.renderRevision === preview.binding.frame.renderRevision &&
      event.event.type !== "selectSource" &&
      event.event.type !== "revealSource"
    )
      preview.binding.pendingNavigation = event.event;
    return true;
  }
  if (
    event.event.type === "selectSource" ||
    event.event.type === "revealSource"
  ) {
    await navigate(runtime, preview, event.event);
    return true;
  }
  if (preview.panel && preview.messageToken)
    await preview.panel.webview.postMessage({
      type: "previewEvent",
      messageToken: preview.messageToken,
      event: event.event,
    });
  return true;
}

export function handlePreviewChangedLifecycle(
  origin: DaemonOrigin,
  changed: PreviewChangedParams,
  runtimes: Iterable<DocumentRuntime>,
  synchronize: (runtime: DocumentRuntime, preview: PreviewState) => void,
): boolean {
  const runtime = [...runtimes].find((candidate) =>
    candidate.previews.has(changed.previewSessionId),
  );
  const preview = runtime?.previews.get(changed.previewSessionId);
  if (
    !runtime ||
    !preview ||
    !preview.binding.active ||
    !sameDaemonOrigin(preview.binding.origin, origin)
  )
    return false;
  if (
    changed.frame.previewSessionId !== preview.binding.previewSessionId ||
    changed.frame.renderRevision !== changed.renderRevision
  )
    return false;
  if (changed.renderRevision <= preview.binding.frame.renderRevision)
    return true;
  preview.binding.frame = changed.frame;
  if (!preview.panel) preview.binding.renderRevision = changed.renderRevision;
  if (preview.panel && preview.webviewReady) synchronize(runtime, preview);
  return true;
}

export interface PreviewEchoState {
  selection?: { uri: string; value: string };
  viewport?: { uri: string };
}
