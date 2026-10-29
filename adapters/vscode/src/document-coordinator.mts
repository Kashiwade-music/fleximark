import type * as vscode from "vscode";

import {
  ConnectionBinding,
  type DaemonConnection,
} from "./daemon-connection.mjs";
import { type JsonRpcConnection, JsonRpcResponseError } from "./rpc.mjs";
import type {
  DocumentRuntime,
  DocumentState,
  PreviewState,
} from "./runtime-state.mjs";

function deactivatePreviewSession(preview: PreviewState): void {
  preview.binding.active = false;
}

export async function syncDocument(
  runtime: DocumentRuntime,
  document: vscode.TextDocument,
  connection: DaemonConnection,
  changeDocument: (document: vscode.TextDocument) => void,
): Promise<void> {
  const uri = document.uri.toString();
  const existing = runtime.documents.get(uri);
  if (existing?.sessionId) return;
  if (existing?.syncing) return existing.syncing;
  const state: DocumentState = existing ?? { version: document.version };
  const binding = new ConnectionBinding(
    connection,
    () => !runtime.removed && runtime.documents.get(uri) === state,
  );
  const operation = (async () => {
    const version = document.version;
    const text = document.getText();
    connection.rpc.notifyLsp("textDocument/didOpen", {
      textDocument: { uri, languageId: document.languageId, version, text },
    });
    const attached = await connection.rpc.request("fleximark/attachDocument", {
      daemonInstanceId: connection.daemonInstanceId,
      uri,
      expectedDocumentVersion: version,
    });
    if (!attached) return;
    if (!binding.isCurrent()) return;
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
  runtime: DocumentRuntime | undefined,
  document: vscode.TextDocument,
  rpc: JsonRpcConnection | undefined,
  synchronize: () => Promise<void>,
  report: (error: unknown) => void,
): void {
  if (document.languageId !== "markdown") return;
  const state = runtime?.documents.get(document.uri.toString());
  if (!state?.sessionId || !runtime || !rpc) {
    if (state && runtime && rpc && !state.syncing)
      void synchronize().catch(report);
    return;
  }
  state.version = document.version;
  rpc.notifyLsp("textDocument/didChange", {
    textDocument: { uri: document.uri.toString(), version: document.version },
    contentChanges: [{ text: document.getText() }],
  });
}

export function closeDocument(
  runtime: DocumentRuntime | undefined,
  document: vscode.TextDocument,
  rpc: JsonRpcConnection | undefined,
): void {
  if (!runtime) return;
  const uri = document.uri.toString();
  rpc?.notifyLsp("textDocument/didClose", { textDocument: { uri } });
  for (const preview of runtime.previews.values())
    if (preview.documentUri === uri) {
      deactivatePreviewSession(preview);
      if (preview.target === "externalBrowser")
        runtime.previews.delete(preview.binding.previewSessionId);
    }
  runtime.documents.delete(uri);
}

export function resetDocumentSessions(
  runtimes: Iterable<DocumentRuntime>,
): void {
  for (const runtime of runtimes) {
    for (const state of runtime.documents.values()) state.sessionId = undefined;
    for (const preview of runtime.previews.values())
      deactivatePreviewSession(preview);
  }
}

export async function replayDocumentSessions(
  documents: Iterable<vscode.TextDocument>,
  sync: (document: vscode.TextDocument) => Promise<void>,
  reject: (document: vscode.TextDocument, error: JsonRpcResponseError) => void,
): Promise<void> {
  for (const document of documents) {
    try {
      await sync(document);
    } catch (error) {
      if (!isEngineDocumentRejection(error)) throw error;
      reject(document, error);
    }
  }
}

function isEngineDocumentRejection(
  error: unknown,
): error is JsonRpcResponseError {
  return (
    error instanceof JsonRpcResponseError &&
    error.code === -32602 &&
    error.data !== null &&
    typeof error.data === "object" &&
    !Array.isArray(error.data) &&
    error.data.kind === "engine"
  );
}
