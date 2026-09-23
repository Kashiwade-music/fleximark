import * as assert from "node:assert/strict";
import { createHash } from "node:crypto";

import {
  type DaemonOrigin,
  type DocumentClock,
  changeDocument,
  checkpointDocument,
  closeDocument,
  replayDocumentSessions,
  resetDocumentSessions,
  syncDocument,
} from "../../adapters/vscode/src/document-coordinator.mjs";
import {
  type JsonRpcConnection,
  JsonRpcResponseError,
} from "../../adapters/vscode/src/rpc.mjs";
import type {
  DocumentState,
  PreviewState,
  WorkspaceRuntime,
} from "../../adapters/vscode/src/runtime-state.mjs";

export const suiteName = "Document coordinator";

export function suite(): void {
  test("opens an already active document with one acknowledged request", async () => {
    const requests: { method: string; params: unknown }[] = [];
    const rpc = {
      closed: false,
      request(method: string, params: unknown) {
        requests.push({ method, params });
        return Promise.resolve({ documentSessionId: "session" });
      },
    } as unknown as JsonRpcConnection;
    const origin: DaemonOrigin = {
      rpc,
      daemonInstanceId: "daemon",
      generation: 1,
    };
    const uri = "file:///document.md";
    const document = textDocument(uri, 1, "already open");
    const runtime = {
      removed: false,
      documents: new Map(),
      previews: new Map(),
    } as unknown as WorkspaceRuntime;

    await syncDocument(
      runtime,
      document,
      origin,
      (owner, opened, state) =>
        owner.documents.get(opened.uri.toString()) === state,
      () => assert.fail("the document did not change while opening"),
    );

    assert.equal(runtime.documents.get(uri)?.sessionId, "session");
    assert.deepEqual(requests, [
      {
        method: "fleximark/openDocument",
        params: {
          daemonInstanceId: "daemon",
          uri,
          documentVersion: 1,
          text: "already open",
        },
      },
    ]);
  });

  test("an old open cannot commit into a closed and reopened document state", async () => {
    let resolveOpen!: (value: { documentSessionId: string }) => void;
    const open = new Promise<{ documentSessionId: string }>((resolve) => {
      resolveOpen = resolve;
    });
    const rpc = {
      closed: false,
      request: () => open,
    } as unknown as JsonRpcConnection;
    const origin: DaemonOrigin = {
      rpc,
      daemonInstanceId: "daemon",
      generation: 1,
    };
    const uri = "file:///document.md";
    const document = textDocument(uri, 1, "first");
    const runtime = {
      removed: false,
      documents: new Map(),
      previews: new Map(),
    } as unknown as WorkspaceRuntime;
    const syncing = syncDocument(
      runtime,
      document,
      origin,
      (owner, opened, state) =>
        owner.documents.get(opened.uri.toString()) === state,
      () => assert.fail("the stale attach must not emit a catch-up change"),
    );
    const oldState = runtime.documents.get(uri);
    assert.ok(oldState?.syncing);
    runtime.documents.delete(uri);
    const reopened: DocumentState = {
      sessionId: "new-session",
      version: 2,
      syncing: Promise.resolve(),
    };
    runtime.documents.set(uri, reopened);

    resolveOpen({ documentSessionId: "old-session" });
    await syncing;
    assert.equal(runtime.documents.get(uri), reopened);
    assert.equal(reopened.sessionId, "new-session");
    assert.notEqual(reopened.syncing, undefined);
    assert.equal(oldState?.sessionId, undefined);
  });

  test("resets the exact 150ms checkpoint and fires with latest text, version, and hash", async () => {
    const scheduled: {
      callback: () => void;
      delay: number;
      handle: NodeJS.Timeout;
    }[] = [];
    const cancelled: NodeJS.Timeout[] = [];
    const clock: DocumentClock = {
      schedule(callback, delay) {
        const handle = { id: scheduled.length } as unknown as NodeJS.Timeout;
        scheduled.push({ callback, delay, handle });
        return handle;
      },
      cancel(handle) {
        cancelled.push(handle);
      },
    };
    const requests: { method: string; params: unknown }[] = [];
    const rpc = {
      closed: false,
      notifyLsp: () => undefined,
      request(method: string, params: unknown) {
        requests.push({ method, params });
        return Promise.resolve(null);
      },
    } as unknown as JsonRpcConnection;
    const origin: DaemonOrigin = {
      rpc,
      daemonInstanceId: "daemon",
      generation: 1,
    };
    const uri = "file:///document.md";
    const state: DocumentState = { sessionId: "session", version: 1 };
    const runtime = {
      documents: new Map([[uri, state]]),
    } as unknown as WorkspaceRuntime;
    const first = textDocument(uri, 2, "first edit");
    const latest = textDocument(uri, 3, "latest edit");
    const fail = (error: unknown) => assert.fail(String(error));
    changeDocument(
      runtime,
      first,
      rpc,
      async () => undefined,
      async () => undefined,
      fail,
      clock,
    );
    changeDocument(
      runtime,
      latest,
      rpc,
      async () => undefined,
      () => checkpointDocument(latest, state, origin),
      fail,
      clock,
    );
    assert.deepEqual(
      scheduled.map(({ delay }) => delay),
      [150, 150],
    );
    assert.deepEqual(cancelled, [scheduled[0].handle]);

    scheduled[1].callback();
    await Promise.resolve();
    assert.deepEqual(requests, [
      {
        method: "fleximark/checkpointDocument",
        params: {
          daemonInstanceId: "daemon",
          documentSessionId: "session",
          documentVersion: 3,
          contentHash: createHash("sha256").update("latest edit").digest("hex"),
        },
      },
    ]);
  });

  test("isolates daemon document rejections during replay but propagates transport failures", async () => {
    const bad = textDocument("file:///bad.md", 1, "bad");
    const good = textDocument("file:///good.md", 1, "good");
    const replayed: string[] = [];
    const reported: unknown[] = [];
    const rejection = new JsonRpcResponseError(
      "engine rejected the document",
      -32602,
      { kind: "engine" },
    );

    await replayDocumentSessions(
      [bad, good],
      async (document) => {
        replayed.push(document.uri.toString());
        if (document === bad) throw rejection;
      },
      (_document, error) => reported.push(error),
    );
    assert.deepEqual(replayed, ["file:///bad.md", "file:///good.md"]);
    assert.deepEqual(reported, [rejection]);

    const transportFailure = new Error("connection closed");
    await assert.rejects(
      replayDocumentSessions(
        [good],
        async () => {
          throw transportFailure;
        },
        () => assert.fail("transport failures must reach the supervisor"),
      ),
      transportFailure,
    );

    for (const error of [
      new JsonRpcResponseError("request cancelled", -32800),
      new JsonRpcResponseError("daemon instance does not match", -32602),
    ])
      await assert.rejects(
        replayDocumentSessions(
          [good],
          async () => {
            throw error;
          },
          () =>
            assert.fail("connection-level errors must reach the supervisor"),
        ),
        error,
      );
  });

  test("retries synchronization when an edited Markdown document has no session", async () => {
    const uri = "file:///rejected.md";
    const document = textDocument(uri, 2, "repaired");
    const runtime = {
      documents: new Map([[uri, { version: 1 }]]),
    } as unknown as WorkspaceRuntime;
    const rpc = { closed: false } as JsonRpcConnection;
    let synchronized = 0;

    changeDocument(
      runtime,
      document,
      rpc,
      async () => {
        synchronized += 1;
      },
      async () => assert.fail("an unopened document cannot checkpoint"),
      (error) => assert.fail(String(error)),
    );
    await Promise.resolve();
    assert.equal(synchronized, 1);
  });

  test("keeps the local preview when its Markdown document closes", () => {
    const uri = "file:///closed.md";
    let panelDisposed = 0;
    const preview = {
      documentUri: uri,
      previewSessionId: "preview",
      remoteSessionActive: true,
      renderRevision: 7,
      readInFlight: Promise.resolve(),
      readAgain: true,
      forceRead: true,
      panel: { dispose: () => panelDisposed++ },
    } as unknown as PreviewState;
    const external = {
      ...preview,
      previewSessionId: "external",
      target: "externalBrowser",
      panel: undefined,
    } as PreviewState;
    const runtime = {
      documents: new Map([[uri, { sessionId: "document", version: 1 }]]),
      previews: new Map([
        [preview.previewSessionId, preview],
        [external.previewSessionId, external],
      ]),
    } as unknown as WorkspaceRuntime;
    const notifications: unknown[] = [];
    const rpc = {
      notifyLsp: (method: string, params: unknown) =>
        void notifications.push({ method, params }),
    } as unknown as JsonRpcConnection;

    closeDocument(runtime, textDocument(uri, 1, "last frame"), rpc);

    assert.deepEqual(notifications, [
      {
        method: "textDocument/didClose",
        params: { textDocument: { uri } },
      },
    ]);
    assert.equal(runtime.documents.has(uri), false);
    assert.equal(runtime.previews.get("preview"), preview);
    assert.equal(runtime.previews.has("external"), false);
    assert.equal(preview.remoteSessionActive, false);
    assert.equal(preview.renderRevision, 7);
    assert.equal(preview.readInFlight, undefined);
    assert.equal(preview.readAgain, false);
    assert.equal(preview.forceRead, false);
    assert.equal(panelDisposed, 0);
  });

  test("disconnects every preview when daemon sessions reset", () => {
    const preview = {
      previewSessionId: "preview",
      remoteSessionActive: true,
      readInFlight: Promise.resolve(),
      readAgain: true,
      forceRead: true,
    } as unknown as PreviewState;
    const documentState = { sessionId: "document", version: 1 };
    const runtime = {
      documents: new Map([["file:///note.md", documentState]]),
      previews: new Map([[preview.previewSessionId, preview]]),
    } as unknown as WorkspaceRuntime;

    resetDocumentSessions([runtime]);

    assert.equal(documentState.sessionId, undefined);
    assert.equal(preview.remoteSessionActive, false);
    assert.equal(preview.readInFlight, undefined);
    assert.equal(preview.readAgain, false);
    assert.equal(preview.forceRead, false);
  });
}

function textDocument(uri: string, version: number, text: string) {
  return {
    languageId: "markdown",
    uri: { toString: () => uri },
    version,
    getText: () => text,
  } as never;
}
