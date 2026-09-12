import { createHash } from "node:crypto";
import type * as vscode from "vscode";

import type { RequestFullTextParams } from "./protocol.mjs";
import type { JsonRpcConnection, JsonRpcRequest } from "./rpc.mjs";
import type { DocumentState, WorkspaceRuntime } from "./runtime-state.mjs";

export interface DaemonOrigin {
  readonly rpc: JsonRpcConnection;
  readonly daemonInstanceId: string;
  readonly generation: number;
}

export interface DocumentClock {
  schedule(callback: () => void, delay: number): NodeJS.Timeout;
  cancel(handle: NodeJS.Timeout): void;
}

const systemDocumentClock: DocumentClock = {
  schedule: (callback, delay) => setTimeout(callback, delay),
  cancel: (handle) => clearTimeout(handle),
};

export async function syncDocument(
  runtime: WorkspaceRuntime,
  document: vscode.TextDocument,
  origin: DaemonOrigin,
  isCurrent: (
    runtime: WorkspaceRuntime,
    document: vscode.TextDocument,
    state: DocumentState,
    origin: DaemonOrigin,
  ) => boolean,
  changeDocument: (document: vscode.TextDocument) => void,
): Promise<void> {
  const uri = document.uri.toString();
  const existing = runtime.documents.get(uri);
  if (existing?.sessionId) return;
  if (existing?.syncing) return existing.syncing;
  const state: DocumentState = existing ?? { version: document.version };
  const operation = (async () => {
    const version = document.version;
    const text = document.getText();
    origin.rpc.notifyLsp("textDocument/didOpen", {
      textDocument: { uri, languageId: document.languageId, version, text },
    });
    const attached = await origin.rpc.request("fleximark/attachDocument", {
      daemonInstanceId: origin.daemonInstanceId,
      uri,
      expectedDocumentVersion: version,
      contentHash: createHash("sha256").update(text).digest("hex"),
    });
    if (!attached) return;
    if (!isCurrent(runtime, document, state, origin)) return;
    state.sessionId = attached.documentSessionId;
    state.version = version;
    if (document.version !== version) changeDocument(document);
  })();
  state.syncing = operation;
  runtime.documents.set(uri, state);
  try {
    await operation;
  } finally {
    if (state.syncing === operation) state.syncing = undefined;
  }
}

export function changeDocument(
  runtime: WorkspaceRuntime | undefined,
  document: vscode.TextDocument,
  rpc: JsonRpcConnection | undefined,
  checkpoint: () => Promise<void>,
  report: (error: unknown) => void,
  clock: DocumentClock = systemDocumentClock,
): void {
  if (document.languageId !== "markdown") return;
  const state = runtime?.documents.get(document.uri.toString());
  if (!state?.sessionId || !runtime || !rpc) return;
  state.version = document.version;
  rpc.notifyLsp("textDocument/didChange", {
    textDocument: { uri: document.uri.toString(), version: document.version },
    contentChanges: [{ text: document.getText() }],
  });
  if (state.checkpoint) clock.cancel(state.checkpoint);
  state.checkpoint = clock.schedule(() => {
    void checkpoint().catch(report);
  }, 150);
}

export function closeDocument(
  runtime: WorkspaceRuntime | undefined,
  document: vscode.TextDocument,
  rpc: JsonRpcConnection | undefined,
  disposePreview: (runtime: WorkspaceRuntime, previewSessionId: string) => void,
): void {
  if (!runtime) return;
  const uri = document.uri.toString();
  const state = runtime.documents.get(uri);
  if (state?.checkpoint) clearTimeout(state.checkpoint);
  rpc?.notifyLsp("textDocument/didClose", { textDocument: { uri } });
  for (const preview of [...runtime.previews.values()])
    if (preview.documentUri === uri)
      disposePreview(runtime, preview.previewSessionId);
  runtime.documents.delete(uri);
}

export async function checkpointDocument(
  document: vscode.TextDocument,
  state: DocumentState,
  origin: DaemonOrigin | undefined,
): Promise<void> {
  if (!origin || origin.rpc.closed || !state.sessionId) return;
  await origin.rpc.request("fleximark/checkpointDocument", {
    daemonInstanceId: origin.daemonInstanceId,
    documentSessionId: state.sessionId,
    documentVersion: document.version,
    contentHash: createHash("sha256").update(document.getText()).digest("hex"),
  });
}

export function resetDocumentSessions(
  runtimes: Iterable<WorkspaceRuntime>,
): void {
  for (const runtime of runtimes)
    for (const state of runtime.documents.values()) state.sessionId = undefined;
}

export function handleRequestFullText(
  message: JsonRpcRequest,
  connection: JsonRpcConnection,
  runtimes: Iterable<WorkspaceRuntime>,
  daemonInstanceId: string | undefined,
  documents: readonly vscode.TextDocument[],
): boolean {
  if (message.method !== "fleximark/requestFullText") return false;
  const params = message.params as RequestFullTextParams;
  const state = [...runtimes]
    .map((runtime) => runtime.documents.get(params.uri))
    .find(Boolean);
  if (
    params.daemonInstanceId !== daemonInstanceId ||
    state?.sessionId !== params.documentSessionId
  )
    return true;
  const document = documents.find((item) => item.uri.toString() === params.uri);
  if (document)
    connection.notifyLsp("textDocument/didChange", {
      textDocument: { uri: params.uri, version: document.version },
      contentChanges: [{ text: document.getText() }],
    });
  if (message.id !== undefined) connection.respond(message.id, null);
  return true;
}
