import * as assert from "node:assert/strict";
import { PassThrough } from "node:stream";
import type * as vscode from "vscode";

import type { DaemonOrigin } from "../../../adapters/vscode/src/document-coordinator.mjs";
import {
  type PreviewLifecycleDependencies,
  disposePreviewCandidate,
  disposePreviewLifecycle,
  embeddedPreviewShell,
  followActiveDocumentLifecycle,
  handlePreviewChangedLifecycle,
  handlePreviewEventLifecycle,
  openPreviewLifecycle,
  previewCandidateIdentityIsCurrent,
  previewCandidateIsCurrent,
  recreatePreviewsLifecycle,
  requestPreviewCandidate,
  rerenderPreviewLifecycle,
  sameDaemonOrigin,
  synchronizePreviewLifecycle,
} from "../../../adapters/vscode/src/preview-coordinator.mjs";
import { JsonRpcConnection } from "../../../adapters/vscode/src/rpc.mjs";
import type {
  DocumentRuntime,
  PreviewState,
} from "../../../adapters/vscode/src/runtime-state.mjs";
import type { RenderFrame } from "../../../contracts/typescript/protocol.mjs";
import { deferred } from "./async-helpers.mjs";

export const suiteName = "Preview coordinator";

export function suite(): void {
  test("keeps a same-chunk pushed frame while opening and rebinding a preview", async () => {
    for (const rebind of [false, true]) {
      const input = new PassThrough();
      const output = new PassThrough();
      const rpc = new JsonRpcConnection(input, output);
      const origin = daemonOrigin(rpc, "daemon", 1);
      const old = previewState(origin, async () => true);
      old.sourceViewColumn = 1;
      const state = { sessionId: "document", version: 1 };
      const runtime = {
        removed: false,
        documents: new Map([["file:///next.md", state]]),
        previews: new Map(rebind ? [["preview", old]] : []),
      } as unknown as DocumentRuntime;
      const document = {
        languageId: "markdown",
        version: 1,
        fileName: "next.md",
        uri: { toString: () => "file:///next.md" },
      } as vscode.TextDocument;
      const messages: unknown[] = [];
      let ready: ((value: unknown) => void) | undefined;
      const panel = {
        webview: {
          cspSource: "resource:",
          postMessage: async (message: unknown) => {
            messages.push(message);
            return true;
          },
          onDidReceiveMessage: (handler: (value: unknown) => void) => {
            ready = handler;
          },
        },
        onDidDispose: () => undefined,
        dispose: () => undefined,
      } as unknown as vscode.WebviewPanel;
      const requested = deferred<undefined>();
      let written = "";
      output.on("data", (chunk: Buffer) => {
        written += chunk.toString();
        if (written.includes("fleximark/createPreview"))
          requested.resolve(undefined);
      });
      const dependencies = {
        activeEditor: () => ({ document, viewColumn: 1 }),
        workspaceFor: () => undefined,
        start: async () => runtime,
        sync: async () => undefined,
        request: (
          owner: DocumentRuntime,
          opened: vscode.TextDocument,
          target: "embeddedHtml",
        ) =>
          requestPreviewCandidate(
            owner,
            opened,
            target,
            () => origin,
            () => true,
            disposePreviewCandidate,
          ),
        candidateCurrent: () => true,
        rejectCandidate: disposePreviewCandidate,
        createPanel: () => panel,
        scriptUri: () => "preview.js",
        random: () => "token",
        css: "",
        log: () => undefined,
        owner: () => runtime,
        previewCurrent: () => true,
        report: (error: unknown) => assert.fail(String(error)),
        synchronize: (
          owner: DocumentRuntime,
          preview: PreviewState,
          force?: boolean,
        ) => synchronizePreviewLifecycle(owner, preview, dependencies, force),
        notifyNavigation: () => undefined,
      } as unknown as PreviewLifecycleDependencies;
      rpc.on("message", (message) =>
        handlePreviewChangedLifecycle(
          origin,
          message.params,
          [runtime],
          (owner, preview) => {
            void dependencies.synchronize(owner, preview);
          },
        ),
      );
      const operation = rebind
        ? followActiveDocumentLifecycle(
            dependencies.activeEditor(),
            [runtime],
            dependencies,
          )
        : openPreviewLifecycle("embeddedHtml", dependencies);
      await requested.promise;
      const encode = (value: unknown) => {
        const body = JSON.stringify(value);
        return Buffer.from(
          `Content-Length: ${Buffer.byteLength(body)}\r\n\r\n${body}`,
        );
      };
      input.write(
        Buffer.concat([
          encode({
            jsonrpc: "2.0",
            id: 1,
            result: {
              previewSessionId: "preview-next",
              frame: { ...frame(1), previewSessionId: "preview-next" },
            },
          }),
          encode({
            jsonrpc: "2.0",
            method: "fleximark/previewChanged",
            params: {
              daemonInstanceId: "daemon",
              previewSessionId: "preview-next",
              renderRevision: 2,
              frame: {
                ...frame(2),
                documentVersion: 1,
                previewSessionId: "preview-next",
              },
            },
          }),
        ]),
      );
      if (rebind) {
        // The old remote disposal follows the committed rebind.
        await new Promise<void>((resolve) => setImmediate(resolve));
        input.write(encode({ jsonrpc: "2.0", id: 2, result: null }));
      }
      await operation;
      if (!rebind) ready?.({ type: "ready" });
      await new Promise<void>((resolve) => setImmediate(resolve));
      const preview = runtime.previews.get("preview-next");
      assert.ok(preview);
      assert.equal(runtime.previews.size, 1);
      assert.equal(preview.binding.frame.renderRevision, 2);
      assert.equal(preview.binding.renderRevision, 2);
      if (!rebind)
        assert.equal(
          (messages[0] as { frame: RenderFrame }).frame.renderRevision,
          2,
        );
      assert.doesNotMatch(written, /readPreview/);
      rpc.close();
    }
  });

  test("removes an uncommitted preview when its connection closes after creation", async () => {
    const input = new PassThrough();
    const output = new PassThrough();
    const rpc = new JsonRpcConnection(input, output);
    const origin = daemonOrigin(rpc, "daemon", 1);
    const document = {
      languageId: "markdown",
      version: 1,
      uri: { toString: () => "file:///note.md" },
    } as vscode.TextDocument;
    const runtime = {
      removed: false,
      documents: new Map([
        ["file:///note.md", { sessionId: "document", version: 1 }],
      ]),
      previews: new Map(),
    } as unknown as DocumentRuntime;
    const pending = requestPreviewCandidate(
      runtime,
      document,
      "embeddedHtml",
      () => origin,
      () => !rpc.closed,
      disposePreviewCandidate,
    );
    const body = JSON.stringify({
      jsonrpc: "2.0",
      id: 1,
      result: { previewSessionId: "preview", frame: frame(1) },
    });
    input.write(`Content-Length: ${Buffer.byteLength(body)}\r\n\r\n${body}`);
    assert.equal(
      runtime.previews.size,
      1,
      "registration is synchronous with the response",
    );
    rpc.close();
    const candidate = await pending;
    assert.ok(candidate);
    await disposePreviewCandidate(candidate);
    assert.equal(runtime.previews.size, 0);
  });

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
      documents: new Map([["file:///next.md", documentState]]),
    } as unknown as DocumentRuntime;
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
    assert.match(shell, /base-uri 'none'/);
    assert.match(shell, /form-action 'none'/);
    assert.match(shell, /script-src 'nonce-nonce' vscode-resource:/);
    assert.match(shell, /img-src[^;]*https:\/\/i\.ytimg\.com/);
    assert.match(
      shell,
      /<meta name="referrer" content="strict-origin-when-cross-origin">/,
    );
    assert.doesNotMatch(shell, /script-src[^;]*'unsafe-inline'/);
    assert.match(shell, /object-src 'none'/);
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
      previewCurrent: (_runtime: DocumentRuntime, item: PreviewState) =>
        runtime.previews.get(item.binding.previewSessionId) === item,
      report: assert.fail,
    };

    const rerendering = rerenderPreviewLifecycle(
      runtime,
      preview,
      dependencies,
    );
    runtime.previews.delete(preview.binding.previewSessionId);
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

  test("delivers pushed frames without reading and fences replaced bindings", async () => {
    const delivery = deferred<boolean>();
    const messages: unknown[] = [];
    const origin = daemonOrigin(
      { closed: false, request: assert.fail } as unknown as JsonRpcConnection,
      "daemon",
      1,
    );
    const preview = previewState(origin, async (message) => {
      messages.push(message);
      return delivery.promise;
    });
    const runtime = runtimeWithPreview(preview);
    const dependencies = { previewCurrent: () => true, report: assert.fail };
    const firstBinding = preview.binding;
    const sending = synchronizePreviewLifecycle(runtime, preview, dependencies);
    preview.binding = { ...firstBinding, renderRevision: 0, frame: frame(2) };
    delivery.resolve(true);
    await sending;
    assert.equal(preview.binding.renderRevision, 0);
    await synchronizePreviewLifecycle(runtime, preview, dependencies);
    assert.equal(preview.binding.renderRevision, 2);
    assert.deepEqual(
      messages.map(
        (message) => (message as { frame: RenderFrame }).frame.renderRevision,
      ),
      [1, 2],
    );
  });

  test("keeps the latest pushed frame until ready and retries failed delivery", async () => {
    const messages: unknown[] = [];
    let delivered = false;
    const origin = daemonOrigin(
      { closed: false, request: assert.fail } as unknown as JsonRpcConnection,
      "daemon",
      1,
    );
    const preview = previewState(origin, async (message) => {
      messages.push(message);
      return delivered;
    });
    preview.webviewReady = false;
    const runtime = runtimeWithPreview(preview);
    const dependencies = { previewCurrent: () => true, report: assert.fail };
    handlePreviewChangedLifecycle(
      origin,
      {
        daemonInstanceId: "daemon",
        previewSessionId: "preview",
        renderRevision: 2,
        frame: frame(2),
      },
      [runtime],
      assert.fail,
    );
    assert.equal(messages.length, 0);
    preview.webviewReady = true;
    await synchronizePreviewLifecycle(runtime, preview, dependencies);
    assert.equal(preview.binding.renderRevision, 0);
    delivered = true;
    await synchronizePreviewLifecycle(runtime, preview, dependencies);
    assert.equal(preview.binding.renderRevision, 2);
    assert.equal(messages.length, 2);
  });

  test("forwards matching navigation after successful frame delivery", async () => {
    const completion = deferred<boolean>();
    const messages: unknown[] = [];
    const origin = daemonOrigin(
      { closed: false } as JsonRpcConnection,
      "daemon",
      1,
    );
    const preview = previewState(origin, async (message) => {
      messages.push(message);
      return messages.length === 1 ? completion.promise : true;
    });
    const runtime = runtimeWithPreview(preview);
    const sending = synchronizePreviewLifecycle(runtime, preview, {
      previewCurrent: () => true,
      report: (error) => assert.fail(String(error)),
    });
    const event = {
      type: "viewport" as const,
      previewSessionId: "preview",
      renderRevision: 1,
      nodeId: "a",
    };
    await handlePreviewEventLifecycle(
      origin,
      {
        daemonInstanceId: "daemon",
        previewSessionId: "preview",
        renderRevision: 1,
        event,
      },
      [runtime],
      async () => assert.fail("must stay in preview"),
    );
    assert.equal(messages.length, 1);
    completion.resolve(true);
    await sending;
    assert.deepEqual(messages[1], {
      type: "previewEvent",
      messageToken: "token",
      event,
    });
  });

  test("reports webview delivery failures", async () => {
    const failure = new Error("webview closed");
    const preview = previewState(
      daemonOrigin({ closed: false } as JsonRpcConnection, "daemon", 1),
      async () => {
        throw failure;
      },
    );
    const reported: unknown[] = [];
    await synchronizePreviewLifecycle(runtimeWithPreview(preview), preview, {
      previewCurrent: () => true,
      report: (error) => reported.push(error),
    });
    assert.deepEqual(reported, [failure]);
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
          frame: frame(9),
        },
        [runtime],
        () => (synchronized = true),
      ),
      false,
    );
    assert.equal(preview.binding.frame.renderRevision, 1);
    assert.equal(synchronized, false);
  });

  test("tracks detached external frame revision before forwarding browser source navigation", async () => {
    const rpc = { closed: false } as JsonRpcConnection;
    const origin = daemonOrigin(rpc, "daemon", 1);
    const document = {
      languageId: "markdown",
      fileName: "note.md",
      uri: { toString: () => "file:///note.md" },
    };
    const documentState = { sessionId: "document", version: 1 };
    const runtime = {
      removed: false,
      documents: new Map([["file:///next.md", documentState]]),
      previews: new Map(),
    } as unknown as DocumentRuntime;
    const opened: string[] = [];
    const dependencies = {
      activeEditor: () => ({ document, viewColumn: 1 }),
      showNoDocument: assert.fail,
      workspaceFor: () => undefined,
      start: async (workspace: unknown) => {
        assert.equal(workspace, undefined);
        return runtime;
      },
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
          frame: frame(1),
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
    assert.equal(preview.binding.renderRevision, 0);

    const navigated: unknown[] = [];
    const sourceNavigation = (renderRevision: number) => ({
      daemonInstanceId: "daemon",
      previewSessionId: "preview",
      renderRevision,
      event: {
        type: "revealSource" as const,
        sourceRange: {
          byteStart: 0,
          byteEnd: 1,
          start: { line: 0, character: 0, encoding: "utf8" as const },
          end: { line: 0, character: 1, encoding: "utf8" as const },
        },
      },
    });
    const navigate = async (
      _owner: DocumentRuntime,
      _preview: PreviewState,
      event: unknown,
    ) => void navigated.push(event);
    await handlePreviewEventLifecycle(
      origin,
      sourceNavigation(1),
      [runtime],
      navigate,
    );
    assert.equal(navigated.length, 1, "scroll works before any document edit");
    assert.equal(preview.binding.renderRevision, 1);

    handlePreviewChangedLifecycle(
      origin,
      {
        daemonInstanceId: "daemon",
        previewSessionId: "preview",
        renderRevision: 3,
        frame: frame(3),
      },
      [runtime],
      assert.fail,
    );
    assert.equal(preview.binding.renderRevision, 3);

    await handlePreviewEventLifecycle(
      origin,
      sourceNavigation(1),
      [runtime],
      navigate,
    );
    assert.equal(
      navigated.length,
      1,
      "old browser frames cannot move the editor",
    );
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
    assert.equal(navigated.length, 2);
  });

  test("forwards editor viewport navigation to the embedded preview", async () => {
    const messages: unknown[] = [];
    const rpc = { closed: false } as JsonRpcConnection;
    const origin = daemonOrigin(rpc, "daemon", 1);
    const preview = previewState(origin, async (message) => {
      messages.push(message);
      return true;
    });
    preview.binding.renderRevision = 3;
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
        async () => assert.fail("viewport navigation must stay in the preview"),
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

  test("moves an embedded preview from a root to another detached Markdown document", async () => {
    const released: unknown[] = [];
    const previousRpc = {
      closed: false,
      request(method: string, params: unknown) {
        released.push({ method, params });
        return Promise.resolve(null);
      },
    } as unknown as JsonRpcConnection;
    const nextRpc = { closed: false } as JsonRpcConnection;
    const previousOrigin = daemonOrigin(previousRpc, "daemon", 1);
    const nextOrigin = daemonOrigin(nextRpc, "daemon", 1);
    const preview = previewState(previousOrigin, async () => true);
    preview.sourceViewColumn = 1;
    preview.binding.renderRevision = 4;
    const previousRuntime = runtimeWithPreview(preview);
    const documentState = { sessionId: "document-next", version: 2 };
    const nextRuntime = {
      removed: false,
      documents: new Map([["file:///next.md", documentState]]),
      previews: new Map(),
    } as unknown as DocumentRuntime;
    const document = {
      languageId: "markdown",
      fileName: "next.md",
      version: 2,
      uri: { toString: () => "file:///next.md" },
    } as vscode.TextDocument;
    const editor = { document, viewColumn: 1 } as vscode.TextEditor;
    const requests: unknown[] = [];
    const synchronized: unknown[] = [];
    const dependencies = {
      activeEditor: () => editor,
      workspaceFor: () => undefined,
      start: async () => nextRuntime,
      sync: async () => void 0,
      request: async (
        runtime: DocumentRuntime,
        requestedDocument: vscode.TextDocument,
      ) => {
        requests.push({ runtime, requestedDocument });
        return {
          origin: nextOrigin,
          runtime: nextRuntime,
          documentUri: "file:///next.md",
          documentState,
          documentSessionId: "document-next",
          documentVersion: 2,
          result: {
            previewSessionId: "preview-next",
            frame: { ...frame(1), previewSessionId: "preview-next" },
          },
        };
      },
      candidateCurrent: () => true,
      rejectCandidate: assert.fail,
      previewCurrent: (runtime: DocumentRuntime, item: PreviewState) =>
        runtime.previews.get(item.binding.previewSessionId) === item,
      synchronize: async (runtime: DocumentRuntime, item: PreviewState) =>
        void synchronized.push({ runtime, item }),
    } as unknown as PreviewLifecycleDependencies;

    preview.sourceViewColumn = 2;
    await followActiveDocumentLifecycle(
      editor,
      [previousRuntime, nextRuntime],
      dependencies,
    );
    assert.equal(requests.length, 0, "another source column is independent");

    preview.sourceViewColumn = 1;
    await followActiveDocumentLifecycle(
      {
        ...editor,
        document: { ...document, languageId: "plaintext" },
      } as vscode.TextEditor,
      [previousRuntime, nextRuntime],
      dependencies,
    );
    assert.equal(requests.length, 0, "non-Markdown editors are ignored");

    await followActiveDocumentLifecycle(
      editor,
      [previousRuntime, nextRuntime],
      dependencies,
    );

    assert.equal(requests.length, 1);
    assert.equal(previousRuntime.previews.size, 0);
    assert.equal(nextRuntime.previews.get("preview-next"), preview);
    assert.equal(preview.documentUri, "file:///next.md");
    assert.equal(preview.binding.active, true);
    assert.equal(preview.binding.renderRevision, 0);
    assert.equal(preview.panel?.title, "FlexiMark: next.md");
    assert.equal(synchronized.length, 1);
    assert.deepEqual(released, [
      {
        method: "fleximark/disposePreview",
        params: {
          daemonInstanceId: "daemon",
          previewSessionId: "preview",
        },
      },
    ]);
  });

  test("reattaches a disconnected panel without disposing its dead session", async () => {
    let released = 0;
    const previousOrigin = daemonOrigin(
      {
        closed: false,
        request: () => {
          released += 1;
          return Promise.resolve(null);
        },
      } as unknown as JsonRpcConnection,
      "daemon",
      1,
    );
    const preview = previewState(previousOrigin, async () => true);
    preview.sourceViewColumn = 1;
    preview.binding.active = false;
    const previousRuntime = runtimeWithPreview(preview);
    const workspace = { uri: { toString: () => "file:///workspace" } };
    const documentState = { sessionId: "next-document", version: 2 };
    const nextRuntime = {
      workspace,
      removed: false,
      documents: new Map([["file:///note.md", documentState]]),
      previews: new Map(),
    } as unknown as DocumentRuntime;
    const document = {
      languageId: "markdown",
      fileName: "note.md",
      version: 2,
      uri: { toString: () => "file:///note.md" },
    } as vscode.TextDocument;
    const editor = { document, viewColumn: 1 } as vscode.TextEditor;
    const dependencies = {
      activeEditor: () => editor,
      workspaceFor: () => workspace,
      start: async () => nextRuntime,
      sync: async () => void 0,
      request: async () => ({
        origin: daemonOrigin(
          { closed: false } as JsonRpcConnection,
          "daemon",
          1,
        ),
        runtime: nextRuntime,
        documentUri: "file:///note.md",
        documentState,
        documentSessionId: "next-document",
        documentVersion: 2,
        result: {
          previewSessionId: "preview-next",
          frame: { ...frame(1), previewSessionId: "preview-next" },
        },
      }),
      candidateCurrent: () => true,
      rejectCandidate: assert.fail,
      previewCurrent: (owner: DocumentRuntime, item: PreviewState) =>
        owner.previews.get(item.binding.previewSessionId) === item,
      synchronize: async () => void 0,
    } as unknown as PreviewLifecycleDependencies;

    await followActiveDocumentLifecycle(
      editor,
      [previousRuntime, nextRuntime],
      dependencies,
    );

    assert.equal(released, 0);
    assert.equal(previousRuntime.previews.size, 0);
    assert.equal(nextRuntime.previews.get("preview-next"), preview);
    assert.equal(preview.binding.active, true);
  });

  test("retains disconnected panels when replay is unavailable", async () => {
    const cases = [
      {
        connection: { closed: false } as JsonRpcConnection,
        daemon: "daemon",
        document: () => undefined,
        request: undefined,
      },
      {
        connection: { closed: true } as JsonRpcConnection,
        daemon: "old-daemon",
        document: () => ({ languageId: "markdown" }) as vscode.TextDocument,
        request: async () => undefined,
      },
    ];
    for (const item of cases) {
      const preview = previewState(
        daemonOrigin(item.connection, item.daemon, 1),
        async () => true,
      );
      if (item.connection.closed) preview.binding.active = false;
      const runtime = runtimeWithPreview(preview);
      await recreatePreviewsLifecycle(runtime, {
        document: item.document,
        request: item.request,
        dispose: async () => assert.fail("the local panel must remain open"),
        report: assert.fail,
      } as unknown as PreviewLifecycleDependencies);
      assert.equal(runtime.previews.get("preview"), preview, item.daemon);
      assert.equal(preview.binding.active, false, item.daemon);
      assert.ok(preview.panel, item.daemon);
    }
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

  test("closes a disconnected panel without disposing its dead remote session", async () => {
    let requested = 0;
    const preview = previewState(
      daemonOrigin(
        {
          closed: false,
          request: () => {
            requested += 1;
            return Promise.reject(new Error("remote preview is gone"));
          },
        } as unknown as JsonRpcConnection,
        "daemon",
        1,
      ),
      async () => true,
    );
    preview.binding.active = false;
    let panelDisposed = 0;
    preview.panel = { dispose: () => panelDisposed++ } as never;
    const runtime = runtimeWithPreview(preview);
    await disposePreviewLifecycle(runtime, preview, assert.fail);
    assert.equal(requested, 0);
    assert.equal(panelDisposed, 1);
    assert.equal(runtime.previews.size, 0);
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
    binding: {
      origin,
      previewSessionId: "preview",
      active: true,
      renderRevision: 0,
      frame: frame(1),
    },
    documentUri: "file:///note.md",
    target: "embeddedHtml",
    webviewReady: true,
    messageToken: "token",
    panel: { webview: { postMessage } } as never,
  };
}

function runtimeWithPreview(preview: PreviewState): DocumentRuntime {
  return {
    removed: false,
    previews: new Map([[preview.binding.previewSessionId, preview]]),
  } as unknown as DocumentRuntime;
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
  };
}
