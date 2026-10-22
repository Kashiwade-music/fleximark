import * as assert from "node:assert/strict";

import {
  type DaemonOrigin,
  changeDocument,
  closeDocument,
  replayDocumentSessions,
  resetDocumentSessions,
  syncDocument,
} from "../../../adapters/vscode/src/document-coordinator.mjs";
import {
  type JsonRpcConnection,
  JsonRpcResponseError,
} from "../../../adapters/vscode/src/rpc.mjs";
import {
  DETACHED_RUNTIME_KEY,
  ensureRuntime,
  reconcileRuntimeMembership,
  runtimeKey,
} from "../../../adapters/vscode/src/runtime-membership.mjs";
import type {
  DocumentRuntime,
  DocumentState,
  PreviewState,
} from "../../../adapters/vscode/src/runtime-state.mjs";

export const suiteName = "Document coordinator";

export function suite(): void {
  test("reclassifies loose documents and previews without replacing sessions or disturbing another root", () => {
    const first = { uri: { toString: () => "file:///first" } } as NonNullable<
      DocumentRuntime["workspace"]
    >;
    const second = { uri: { toString: () => "file:///second" } } as NonNullable<
      DocumentRuntime["workspace"]
    >;
    const runtimes = new Map<string, DocumentRuntime>();
    const detached = ensureRuntime(runtimes);
    const other = ensureRuntime(runtimes, second);
    const state = { sessionId: "loose-session", version: 3 };
    const preview = {
      documentUri: "file:///first/loose.md",
      binding: { previewSessionId: "preview" },
    } as PreviewState;
    detached.documents.set(preview.documentUri, state);
    detached.previews.set("preview", preview);
    other.documents.set("file:///second/kept.md", {
      sessionId: "other-session",
      version: 1,
    });
    assert.equal(runtimeKey(), DETACHED_RUNTIME_KEY);
    reconcileRuntimeMembership(runtimes, (uri) =>
      uri.startsWith("file:///first/") ? first : second,
    );
    const moved = runtimes.get(runtimeKey(first));
    assert.equal(moved?.documents.get(preview.documentUri), state);
    assert.equal(moved?.previews.get("preview"), preview);
    assert.equal(detached.documents.size, 0);
    reconcileRuntimeMembership(
      runtimes,
      (uri) => (uri.startsWith("file:///first/") ? first : second),
      runtimeKey(first),
    );
    assert.equal(runtimes.has(runtimeKey(first)), false);
    assert.equal(detached.documents.get(preview.documentUri), state);
    assert.equal(detached.previews.get("preview"), preview);
    assert.equal(
      other.documents.get("file:///second/kept.md")?.sessionId,
      "other-session",
    );
    assert.equal(
      [...runtimes.values()].filter((runtime) =>
        runtime.documents.has(preview.documentUri),
      ).length,
      1,
    );
  });
  test("opens an already active document with one acknowledged request", async () => {
    const requests: { method: string; params: unknown }[] = [];
    const rpc = {
      closed: false,
      notifyLsp(method: string, params: unknown) {
        requests.push({ method, params });
      },
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
    } as unknown as DocumentRuntime;

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
        method: "textDocument/didOpen",
        params: {
          textDocument: {
            uri,
            languageId: "markdown",
            version: 1,
            text: "already open",
          },
        },
      },
      {
        method: "fleximark/attachDocument",
        params: {
          daemonInstanceId: "daemon",
          uri,
          expectedDocumentVersion: 1,
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
      notifyLsp: () => undefined,
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
    } as unknown as DocumentRuntime;
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

  test("sends complete text immediately for every document version", () => {
    const uri = "file:///document.md";
    const state: DocumentState = { sessionId: "session", version: 1 };
    const runtime = {
      documents: new Map([[uri, state]]),
    } as unknown as DocumentRuntime;
    const changes: unknown[] = [];
    const rpc = {
      notifyLsp(method: string, params: unknown) {
        changes.push({ method, params });
      },
      request: assert.fail,
    } as unknown as JsonRpcConnection;
    for (const [version, text] of [
      [2, "first"],
      [3, "latest 😀"],
    ] as const)
      changeDocument(
        runtime,
        textDocument(uri, version, text),
        rpc,
        async () => assert.fail("already synchronized"),
        (error) => assert.fail(String(error)),
      );
    assert.deepEqual(changes, [
      {
        method: "textDocument/didChange",
        params: {
          textDocument: { uri, version: 2 },
          contentChanges: [{ text: "first" }],
        },
      },
      {
        method: "textDocument/didChange",
        params: {
          textDocument: { uri, version: 3 },
          contentChanges: [{ text: "latest 😀" }],
        },
      },
    ]);
    assert.equal(state.version, 3);
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
    } as unknown as DocumentRuntime;
    const rpc = { closed: false } as JsonRpcConnection;
    let synchronized = 0;

    changeDocument(
      runtime,
      document,
      rpc,
      async () => {
        synchronized += 1;
      },
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
      binding: { previewSessionId: "preview", active: true, renderRevision: 7 },
      panel: { dispose: () => panelDisposed++ },
    } as unknown as PreviewState;
    const external = {
      ...preview,
      binding: { ...preview.binding, previewSessionId: "external" },
      target: "externalBrowser",
      panel: undefined,
    } as PreviewState;
    const runtime = {
      documents: new Map([[uri, { sessionId: "document", version: 1 }]]),
      previews: new Map([
        [preview.binding.previewSessionId, preview],
        [external.binding.previewSessionId, external],
      ]),
    } as unknown as DocumentRuntime;
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
    assert.equal(preview.binding.active, false);
    assert.equal(preview.binding.renderRevision, 7);
    assert.equal(panelDisposed, 0);
  });

  test("disconnects every preview when daemon sessions reset", () => {
    const preview = {
      binding: { previewSessionId: "preview", active: true },
    } as unknown as PreviewState;
    const documentState = { sessionId: "document", version: 1 };
    const runtime = {
      documents: new Map([["file:///note.md", documentState]]),
      previews: new Map([[preview.binding.previewSessionId, preview]]),
    } as unknown as DocumentRuntime;

    resetDocumentSessions([runtime]);

    assert.equal(documentState.sessionId, undefined);
    assert.equal(preview.binding.active, false);
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
