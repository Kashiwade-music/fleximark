import * as assert from "node:assert/strict";

import type { DaemonOrigin } from "../../adapters/vscode/src/document-coordinator.mjs";
import {
  type PreviewLifecycleDependencies,
  disposePreviewLifecycle,
  embeddedPreviewShell,
  handlePreviewChangedLifecycle,
  handlePreviewEventLifecycle,
  openPreviewLifecycle,
  previewCandidateIdentityIsCurrent,
  previewCandidateIsCurrent,
  rerenderPreviewLifecycle,
  sameDaemonOrigin,
  synchronizePreviewLifecycle,
} from "../../adapters/vscode/src/preview-coordinator.mjs";
import type { RenderFrame } from "../../adapters/vscode/src/protocol.mjs";
import type { JsonRpcConnection } from "../../adapters/vscode/src/rpc.mjs";
import type {
  PreviewState,
  WorkspaceRuntime,
} from "../../adapters/vscode/src/runtime-state.mjs";
import { deferred } from "./async-helpers.mjs";

export const suiteName = "Preview coordinator";

export function suite(): void {
  test("compares every daemon origin identity dimension", () => {
    const rpc = {} as JsonRpcConnection;
    const origin = daemonOrigin(rpc, "daemon", 3);
    assert.equal(sameDaemonOrigin(origin, origin), true);
    assert.equal(sameDaemonOrigin({ ...origin, generation: 4 }, origin), false);
    assert.equal(
      sameDaemonOrigin({ ...origin, daemonInstanceId: "other" }, origin),
      false,
    );
    assert.equal(
      sameDaemonOrigin(
        daemonOrigin({} as JsonRpcConnection, "daemon", 3),
        origin,
      ),
      false,
    );
  });

  test("rejects stale create candidates before committing UI", () => {
    const rpc = { closed: false } as JsonRpcConnection;
    const origin = daemonOrigin(rpc, "daemon", 1);
    const documentState = { sessionId: "document", version: 4 };
    const runtime = {
      removed: false,
      documents: new Map([["file:///note.md", documentState]]),
    } as unknown as WorkspaceRuntime;
    const candidate = {
      origin,
      runtime,
      documentUri: "file:///note.md",
      documentState,
      documentSessionId: "document",
      documentVersion: 4,
    };
    const current = {
      disposed: false,
      runtime,
      origin,
      documentState,
      documentVersion: 4,
    };
    assert.equal(previewCandidateIsCurrent(candidate, current), true);
    assert.equal(
      previewCandidateIsCurrent(candidate, { ...current, documentVersion: 5 }),
      false,
    );
    assert.equal(
      previewCandidateIdentityIsCurrent(candidate, {
        ...current,
        origin: { ...origin, generation: 2 },
      }),
      false,
    );
  });

  test("builds a CSP shell without embedding frame data", () => {
    const shell = embeddedPreviewShell(
      "nonce",
      "token",
      "vscode-resource:",
      "preview.js",
      "body{}",
    );
    assert.match(shell, /default-src 'none'/);
    assert.match(shell, /fleximark-message-token/);
    assert.doesNotMatch(shell, /renderRevision|rendererFingerprint|blocks/);
  });

  test("rerenders the current preview and rejects stale completion", async () => {
    const response = deferred<null>();
    const calls: { method: string; params: unknown }[] = [];
    const rpc = {
      closed: false,
      request(method: string, params: unknown) {
        calls.push({ method, params });
        return response.promise;
      },
    } as unknown as JsonRpcConnection;
    const origin = daemonOrigin(rpc, "daemon", 1);
    const preview = previewState(origin, async () => true);
    const runtime = runtimeWithPreview(preview);
    const dependencies = {
      previewCurrent: (_runtime: WorkspaceRuntime, item: PreviewState) =>
        runtime.previews.get(item.previewSessionId) === item,
      report: assert.fail,
    };

    const rerendering = rerenderPreviewLifecycle(
      runtime,
      preview,
      dependencies,
    );
    runtime.previews.delete(preview.previewSessionId);
    response.resolve(null);

    assert.equal(await rerendering, false);
    assert.deepEqual(calls, [
      {
        method: "fleximark/rerenderPreview",
        params: {
          daemonInstanceId: "daemon",
          previewSessionId: "preview",
        },
      },
    ]);
  });

  test("coalesces notifications received during a read and applies only latest frames", async () => {
    const first = deferred<{ frame: RenderFrame | null }>();
    const second = deferred<{ frame: RenderFrame | null }>();
    const calls: unknown[] = [];
    const messages: unknown[] = [];
    const rpc = {
      closed: false,
      request(_method: string, params: unknown) {
        calls.push(params);
        return calls.length === 1 ? first.promise : second.promise;
      },
    } as unknown as JsonRpcConnection;
    const origin = daemonOrigin(rpc, "daemon", 1);
    const preview = previewState(origin, async (message) => {
      messages.push(message);
      return true;
    });
    const runtime = runtimeWithPreview(preview);
    const dependencies = {
      previewCurrent: (_runtime: WorkspaceRuntime, item: PreviewState) =>
        runtime.previews.get(item.previewSessionId) === item,
      report: assert.fail,
    };
    const reading = synchronizePreviewLifecycle(
      runtime,
      preview,
      dependencies,
      true,
    );
    handlePreviewChangedLifecycle(
      origin,
      {
        daemonInstanceId: "daemon",
        previewSessionId: "preview",
        renderRevision: 2,
      },
      [runtime],
      (owner, item) =>
        void synchronizePreviewLifecycle(owner, item, dependencies),
    );
    first.resolve({ frame: frame(1) });
    await Promise.resolve();
    second.resolve({ frame: frame(2) });
    await reading;

    assert.equal(calls.length, 2);
    assert.deepEqual(calls[0], {
      daemonInstanceId: "daemon",
      previewSessionId: "preview",
    });
    assert.deepEqual(calls[1], {
      daemonInstanceId: "daemon",
      previewSessionId: "preview",
      afterRevision: 1,
    });
    assert.deepEqual(
      messages.map(
        (message) => (message as { frame: RenderFrame }).frame.renderRevision,
      ),
      [1, 2],
    );
    assert.equal(preview.renderRevision, 2);
  });

  test("drops a read response from a replaced connection incarnation", async () => {
    const response = deferred<{ frame: RenderFrame | null }>();
    let posted = false;
    const firstRpc = {
      closed: false,
      request: () => response.promise,
    } as unknown as JsonRpcConnection;
    const preview = previewState(
      daemonOrigin(firstRpc, "old", 1),
      async () => ((posted = true), true),
    );
    const runtime = runtimeWithPreview(preview);
    const reading = synchronizePreviewLifecycle(
      runtime,
      preview,
      { previewCurrent: () => true, report: assert.fail },
      true,
    );
    await Promise.resolve();
    preview.origin = daemonOrigin(
      { closed: false } as JsonRpcConnection,
      "new",
      2,
    );
    response.resolve({ frame: frame(1) });
    await reading;
    assert.equal(posted, false);
    assert.equal(preview.renderRevision, 0);
  });

  test("ignores changed notifications from stale origins", () => {
    const rpc = { closed: false } as JsonRpcConnection;
    const preview = previewState(
      daemonOrigin(rpc, "current", 2),
      async () => true,
    );
    const runtime = runtimeWithPreview(preview);
    let synchronized = false;
    assert.equal(
      handlePreviewChangedLifecycle(
        daemonOrigin(rpc, "old", 1),
        {
          daemonInstanceId: "old",
          previewSessionId: "preview",
          renderRevision: 9,
        },
        [runtime],
        () => (synchronized = true),
      ),
      false,
    );
    assert.equal(preview.notifiedRevision, 0);
    assert.equal(synchronized, false);
  });

  test("tracks external frame revision before forwarding browser source navigation", async () => {
    const rpc = { closed: false } as JsonRpcConnection;
    const origin = daemonOrigin(rpc, "daemon", 1);
    const document = {
      languageId: "markdown",
      fileName: "note.md",
      uri: { toString: () => "file:///note.md" },
    };
    const workspace = { uri: { toString: () => "file:///workspace" } };
    const documentState = { sessionId: "document", version: 1 };
    const runtime = {
      removed: false,
      documents: new Map([["file:///note.md", documentState]]),
      previews: new Map(),
    } as unknown as WorkspaceRuntime;
    const opened: string[] = [];
    const dependencies = {
      activeEditor: () => ({ document, viewColumn: 1 }),
      showNoDocument: assert.fail,
      showNoWorkspace: assert.fail,
      workspaceFor: () => workspace,
      start: async () => runtime,
      sync: async () => void 0,
      request: async () => ({
        origin,
        runtime,
        documentUri: "file:///note.md",
        documentState,
        documentSessionId: "document",
        documentVersion: 1,
        result: {
          previewSessionId: "preview",
          url: "http://127.0.0.1/preview",
        },
      }),
      candidateCurrent: () => true,
      rejectCandidate: async () => void 0,
      synchronize: async () => void 0,
      openExternal: async (url: string) => void opened.push(url),
      createPanel: assert.fail,
      scriptUri: assert.fail,
      random: assert.fail,
      css: "",
      log: () => void 0,
      previewCurrent: () => true,
      report: assert.fail,
      dispose: async () => void 0,
      notifyNavigation: () => void 0,
    } as unknown as PreviewLifecycleDependencies;

    await openPreviewLifecycle("externalBrowser", dependencies);
    const preview = runtime.previews.get("preview");
    assert.ok(preview);
    assert.deepEqual(opened, ["http://127.0.0.1/preview"]);
    assert.equal(preview.renderRevision, 0);

    handlePreviewChangedLifecycle(
      origin,
      {
        daemonInstanceId: "daemon",
        previewSessionId: "preview",
        renderRevision: 3,
      },
      [runtime],
      assert.fail,
    );
    assert.equal(preview.renderRevision, 3);

    const navigated: unknown[] = [];
    assert.equal(
      await handlePreviewEventLifecycle(
        origin,
        {
          daemonInstanceId: "daemon",
          previewSessionId: "preview",
          renderRevision: 3,
          event: {
            type: "selectSource",
            sourceRange: {
              byteStart: 0,
              byteEnd: 1,
              start: { line: 0, character: 0, encoding: "utf8" },
              end: { line: 0, character: 1, encoding: "utf8" },
            },
          },
        },
        [runtime],
        async (_owner, _preview, event) => void navigated.push(event),
      ),
      true,
    );
    assert.equal(navigated.length, 1);
  });

  test("forwards editor viewport navigation to the embedded preview", async () => {
    const messages: unknown[] = [];
    const rpc = { closed: false } as JsonRpcConnection;
    const origin = daemonOrigin(rpc, "daemon", 1);
    const preview = previewState(origin, async (message) => {
      messages.push(message);
      return true;
    });
    preview.renderRevision = 3;
    const runtime = runtimeWithPreview(preview);

    assert.equal(
      await handlePreviewEventLifecycle(
        origin,
        {
          daemonInstanceId: "daemon",
          previewSessionId: "preview",
          renderRevision: 3,
          event: {
            type: "viewport",
            previewSessionId: "preview",
            renderRevision: 3,
            nodeId: "section-2",
          },
        },
        [runtime],
        assert.fail,
      ),
      true,
    );
    assert.deepEqual(messages, [
      {
        type: "previewEvent",
        messageToken: "token",
        event: {
          type: "viewport",
          previewSessionId: "preview",
          renderRevision: 3,
          nodeId: "section-2",
        },
      },
    ]);
  });

  test("removes membership before awaiting daemon disposal", async () => {
    const disposed = deferred<null>();
    const rpc = {
      closed: false,
      request: () => disposed.promise,
    } as unknown as JsonRpcConnection;
    const preview = previewState(
      daemonOrigin(rpc, "daemon", 1),
      async () => true,
    );
    let panelDisposed = 0;
    preview.panel = {
      ...preview.panel,
      dispose: () => panelDisposed++,
    } as never;
    const runtime = runtimeWithPreview(preview);
    const operation = disposePreviewLifecycle(runtime, preview, assert.fail);
    assert.equal(runtime.previews.has("preview"), false);
    disposed.resolve(null);
    await operation;
    assert.equal(panelDisposed, 1);
  });
}

function daemonOrigin(
  rpc: JsonRpcConnection,
  daemonInstanceId: string,
  generation: number,
): DaemonOrigin {
  return { rpc, daemonInstanceId, generation };
}

function previewState(
  origin: DaemonOrigin,
  postMessage: (message: unknown) => Promise<boolean>,
): PreviewState {
  return {
    origin,
    documentUri: "file:///note.md",
    previewSessionId: "preview",
    target: "embeddedHtml",
    renderRevision: 0,
    notifiedRevision: 0,
    webviewReady: true,
    messageToken: "token",
    panel: { webview: { postMessage } } as never,
  };
}

function runtimeWithPreview(preview: PreviewState): WorkspaceRuntime {
  return {
    removed: false,
    previews: new Map([[preview.previewSessionId, preview]]),
  } as unknown as WorkspaceRuntime;
}

function frame(renderRevision: number): RenderFrame {
  return {
    previewSessionId: "preview",
    documentVersion: renderRevision,
    renderRevision,
    rendererFingerprint: "a".repeat(64),
    style: null,
    assets: [],
    blocks: [
      {
        id: "a",
        html: '<p data-fleximark-node-id="a">frame</p>',
        nodeIds: ["a"],
      },
    ],
    navigation: [],
    annotations: {},
  };
}
