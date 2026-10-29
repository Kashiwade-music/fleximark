import { embeddedPreviewShell } from "fleximark/src/preview-coordinator";
import type { DocumentRuntime } from "fleximark/src/runtime-state";
import * as assert from "node:assert/strict";

import { deferred, flushMicrotasks } from "../support/async-helpers.mjs";
import { until } from "../support/polling.mjs";
import {
  PreviewHarness,
  deliveredFrames,
  renderFrame,
} from "../support/preview-harness.mjs";

export const suiteName = "Preview controller";

export function suite(): void {
  test("keeps same-chunk pushed frames while opening and rebinding", async () => {
    const h = new PreviewHarness();
    let sequence = 0;
    h.onRequest = (request, respond) => {
      if (request.method !== "fleximark/createPreview") return false;
      const id = `preview-${++sequence}`;
      respond({ previewSessionId: id, frame: renderFrame(1, id) }, [
        h.changed(renderFrame(2, id)),
      ]);
      return true;
    };
    try {
      const { panel, preview } = await h.open();
      panel.ready();
      await until(
        () => deliveredFrames(panel).at(-1)?.renderRevision === 2,
        "same chunk frame",
      );
      const editor = h.nextDocument();
      await h.controller.follow(editor);
      assert.equal(h.vscode.panels.length, 1);
      assert.equal(preview.documentUri, editor.document.uri.toString());
      assert.equal(deliveredFrames(panel).at(-1)?.renderRevision, 2);
      assert.equal(
        deliveredFrames(panel).at(-1)?.previewSessionId,
        "preview-2",
      );
    } finally {
      h.dispose();
    }
  });

  test("removes an uncommitted preview when its connection closes after creation", async () => {
    const h = new PreviewHarness();
    h.onRequest = (request, respond) => {
      if (request.method !== "fleximark/createPreview") return false;
      respond({ previewSessionId: "dead", frame: renderFrame(1, "dead") });
      h.connection?.dispose();
      return true;
    };
    try {
      await h.controller.open("embeddedHtml");
      assert.equal(h.runtime.previews.size, 0);
      assert.equal(h.vscode.panels.length, 0);
    } finally {
      h.dispose();
    }
  });

  test("connection identity is immutable and disposal aborts pending work first", async () => {
    const h = new PreviewHarness();
    try {
      const connection = h.connection;
      assert.ok(connection);
      assert.throws(
        () => connection.identify("replacement"),
        /identity cannot change/,
      );
      h.held.add("textDocument/hover");
      const pending = connection.rpc
        .requestLsp("textDocument/hover", {})
        .catch((error: unknown) => error);
      let abortedWhenClosed = false;
      connection.rpc.once("close", () => {
        abortedWhenClosed = connection.signal.aborted;
      });
      connection.dispose();
      connection.dispose();
      assert.equal(abortedWhenClosed, true);
      assert.ok((await pending) instanceof Error);
      const next = h.replaceConnection();
      assert.notEqual(next, connection);
      assert.notEqual(next.generation, connection.generation);
      assert.notEqual(next.daemonInstanceId, connection.daemonInstanceId);
    } finally {
      h.dispose();
    }
  });

  test("rejects stale create results before committing UI", async () => {
    for (const mutation of [
      "version",
      "document",
      "session",
      "membership",
    ] as const) {
      const h = new PreviewHarness();
      h.onRequest = (request, respond) => {
        if (request.method !== "fleximark/createPreview") return false;
        const state = h.runtime.documents.get(h.vscode.document.uri.toString());
        assert.ok(state);
        if (mutation === "version") h.vscode.document.version += 1;
        if (mutation === "document")
          h.runtime.documents.delete(h.vscode.document.uri.toString());
        if (mutation === "session") state.sessionId = undefined;
        if (mutation === "membership") h.runtime.removed = true;
        respond({ previewSessionId: "stale", frame: renderFrame(1, "stale") });
        return true;
      };
      try {
        await h.controller.open("embeddedHtml");
        assert.equal(h.vscode.panels.length, 0, mutation);
        assert.equal(h.runtime.previews.size, 0, mutation);
      } finally {
        h.dispose();
      }
    }
  });

  test("builds a CSP shell without embedding frame data", () => {
    const shell = embeddedPreviewShell(
      "nonce",
      "token",
      "resource:",
      "resource:/script.js",
      ".theme{}",
    );
    assert.match(shell, /default-src 'none'/);
    assert.match(shell, /script-src 'nonce-nonce' resource:/);
    assert.match(shell, /name="fleximark-message-token" content="token"/);
    assert.match(shell, /font-src data:/);
    assert.match(shell, /media-src resource: blob:/);
    assert.match(shell, /frame-src https:\/\/www.youtube-nocookie.com/);
    assert.match(shell, /object-src 'none'/);
    assert.match(shell, /class="markdown-body"/);
    assert.doesNotMatch(
      shell,
      /previewFrame|rendererFingerprint|renderRevision/,
    );
  });

  test("rerenders only a current binding and fences stale completion", async () => {
    const h = new PreviewHarness();
    try {
      const { preview } = await h.open();
      assert.equal(await h.controller.rerender(preview), true);
      h.held.add("fleximark/rerenderPreview");
      const pending = h.controller.rerender(preview);
      await h.controller.release(preview);
      h.pending[0].respond(null);
      assert.equal(await pending, false);
    } finally {
      h.dispose();
    }
  });

  test("delivers pushed frames without reading and fences replaced bindings", async () => {
    const h = new PreviewHarness();
    try {
      const { panel, preview } = await h.open();
      const posted = deferred<boolean>();
      panel.webview.postMessage = () => posted.promise;
      panel.ready();
      const old = preview.binding;
      const editor = h.nextDocument();
      const moving = h.controller.follow(editor);
      await flushMicrotasks();
      posted.resolve(true);
      await moving;
      assert.notEqual(preview.binding, old);
      assert.equal(old.renderRevision, 0);
      assert.equal(
        h.requests.some(({ method }) => /getFrame|readPreview/.test(method)),
        false,
      );
    } finally {
      h.dispose();
    }
  });

  test("keeps the latest frame until ready and recovers one rejected delivery", async () => {
    const h = new PreviewHarness();
    try {
      const { panel, preview } = await h.open();
      h.push(renderFrame(2, preview.binding.previewSessionId));
      assert.equal(deliveredFrames(panel).length, 0);
      const send = panel.webview.postMessage;
      let calls = 0;
      panel.webview.postMessage = async (message) =>
        ++calls === 1 ? false : send(message);
      panel.ready();
      await until(() => preview.binding.renderRevision === 3, "recovery frame");
      assert.equal(deliveredFrames(panel).at(-1)?.renderRevision, 3);
      assert.equal(h.errors.length, 0);
    } finally {
      h.dispose();
    }
  });

  test("a replaced delivery session cannot commit its late revision", async () => {
    const h = new PreviewHarness();
    try {
      const { panel, preview } = await h.open();
      const first = deferred<boolean>();
      const send = panel.webview.postMessage;
      let calls = 0;
      panel.webview.postMessage = async (message) => {
        if (++calls === 1) await first.promise;
        return send(message);
      };
      panel.ready();
      h.push(renderFrame(2, preview.binding.previewSessionId));
      await h.controller.deliver(preview, true);
      assert.equal(preview.binding.renderRevision, 2);
      first.resolve(true);
      await flushMicrotasks();
      assert.equal(preview.binding.renderRevision, 2);
    } finally {
      h.dispose();
    }
  });

  test("forwards matching navigation after successful asynchronous frame delivery", async () => {
    const h = new PreviewHarness();
    try {
      const { panel, preview } = await h.open();
      const posted = deferred<boolean>();
      const send = panel.webview.postMessage;
      panel.webview.postMessage = async (message) => {
        if ((message as { type: string }).type === "previewFrame")
          await posted.promise;
        return send(message);
      };
      panel.ready();
      const event = {
        type: "viewport" as const,
        nodeId: "node",
        previewSessionId: preview.binding.previewSessionId,
        renderRevision: 1,
      };
      const navigation = h.controller.event(h.activeConnection, {
        daemonInstanceId: h.activeConnection.daemonInstanceId,
        previewSessionId: event.previewSessionId,
        renderRevision: 1,
        event,
      });
      assert.equal(panel.messages.length, 0);
      posted.resolve(true);
      await navigation;
      assert.deepEqual(
        panel.messages.map((message) => (message as { type: string }).type),
        ["previewFrame", "previewEvent"],
      );
    } finally {
      h.dispose();
    }
  });

  test("reports delivery failure after the one permitted recovery", async () => {
    const h = new PreviewHarness();
    try {
      const { panel } = await h.open();
      panel.webview.postMessage = async () => {
        throw new Error("delivery failed");
      };
      panel.ready();
      await until(() => h.errors.length === 1, "bounded delivery error");
      assert.equal(
        h.requests.filter(
          ({ method }) => method === "fleximark/rerenderPreview",
        ).length,
        1,
      );
      assert.match(String(h.errors[0]), /delivery failed/);
    } finally {
      h.dispose();
    }
  });

  test("ignores changed notifications from a stale daemon identity", async () => {
    const h = new PreviewHarness();
    try {
      const { preview } = await h.open();
      const before = preview.binding.frame;
      h.controller.frame(h.activeConnection, {
        daemonInstanceId: "old-daemon",
        previewSessionId: before.previewSessionId,
        renderRevision: 2,
        frame: renderFrame(2, before.previewSessionId),
      });
      assert.equal(preview.binding.frame, before);
    } finally {
      h.dispose();
    }
  });

  test("keeps only external revision and validates browser source navigation against it", async () => {
    const h = new PreviewHarness();
    try {
      await h.controller.open("externalBrowser");
      const preview = [...h.runtime.previews.values()][0];
      assert.equal(preview.target, "externalBrowser");
      assert.equal("frame" in preview.binding, false);
      const frame = renderFrame(2, preview.binding.previewSessionId);
      h.push(frame);
      const params = {
        daemonInstanceId: h.activeConnection.daemonInstanceId,
        previewSessionId: frame.previewSessionId,
        renderRevision: 2,
        event: {
          type: "selectSource" as const,
          sourceRange: frame.navigation[0].sourceRange,
        },
      };
      await h.controller.event(h.activeConnection, {
        ...params,
        renderRevision: 1,
      });
      assert.equal(h.vscode.editor.reveals.length, 0);
      await h.controller.event(h.activeConnection, params);
      assert.equal(h.vscode.editor.reveals.length, 1);
      assert.equal(preview.binding.renderRevision, 2);
      assert.equal("frame" in preview.binding, false);
    } finally {
      h.dispose();
    }
  });

  test("forwards editor viewport navigation to the embedded preview", async () => {
    const h = new PreviewHarness();
    try {
      const { panel, preview } = await h.open();
      panel.ready();
      await until(() => preview.binding.renderRevision === 1, "viewport frame");
      const event = {
        type: "viewport" as const,
        nodeId: "node",
        previewSessionId: preview.binding.previewSessionId,
        renderRevision: 1,
      };
      await h.controller.event(h.activeConnection, {
        daemonInstanceId: h.activeConnection.daemonInstanceId,
        previewSessionId: event.previewSessionId,
        renderRevision: 1,
        event,
      });
      assert.deepEqual(
        (panel.messages.at(-1) as { event: unknown }).event,
        event,
      );
    } finally {
      h.dispose();
    }
  });

  test("moves an embedded preview from its root to detached Markdown", async () => {
    const h = new PreviewHarness();
    try {
      const { panel, preview } = await h.open();
      panel.ready();
      const detached: DocumentRuntime = {
        removed: false,
        documents: new Map(),
        previews: new Map(),
      };
      h.runtimes.set("detached", detached);
      h.destination = detached;
      const editor = h.nextDocument();
      await h.controller.follow(editor);
      assert.equal(h.runtime.previews.size, 0);
      assert.equal(
        detached.previews.get(preview.binding.previewSessionId),
        preview,
      );
      assert.equal(preview.panel, panel);
      assert.equal(preview.documentUri, editor.document.uri.toString());
    } finally {
      h.dispose();
    }
  });

  test("reattaches a disconnected panel without disposing its dead remote session", async () => {
    const h = new PreviewHarness();
    try {
      const { panel, preview } = await h.open();
      panel.ready();
      const original = preview.binding;
      h.replaceConnection();
      await h.controller.replay();
      assert.equal(preview.panel, panel);
      assert.notEqual(preview.binding, original);
      assert.equal(
        h.requests.filter(({ method }) => method === "fleximark/disposePreview")
          .length,
        0,
      );
    } finally {
      h.dispose();
    }
  });

  test("retains disconnected panels when replay is unavailable", async () => {
    const h = new PreviewHarness();
    try {
      const { panel, preview } = await h.open();
      h.connection?.dispose();
      h.connection = undefined;
      assert.equal(await h.controller.replay(), 0);
      assert.equal(panel.disposed, false);
      assert.equal(
        h.runtime.previews.get(preview.binding.previewSessionId),
        preview,
      );
    } finally {
      h.dispose();
    }
  });

  test("removes membership before awaiting remote disposal", async () => {
    const h = new PreviewHarness();
    try {
      const { panel, preview } = await h.open();
      h.held.add("fleximark/disposePreview");
      const disposing = h.controller.release(preview);
      assert.equal(h.runtime.previews.size, 0);
      assert.equal(preview.binding.isCurrent(), false);
      assert.equal(panel.disposed, true);
      assert.equal(h.pending.length, 1);
      h.pending[0].respond(null);
      await disposing;
      assert.equal(
        h.requests.filter(({ method }) => method === "fleximark/disposePreview")
          .length,
        1,
      );
    } finally {
      h.dispose();
    }
  });

  test("closes disconnected panels without disposing dead remote sessions", async () => {
    const h = new PreviewHarness();
    try {
      const { panel } = await h.open();
      h.connection?.dispose();
      panel.dispose();
      await flushMicrotasks();
      assert.equal(h.runtime.previews.size, 0);
      assert.equal(
        h.requests.filter(({ method }) => method === "fleximark/disposePreview")
          .length,
        0,
      );
    } finally {
      h.dispose();
    }
  });
}
