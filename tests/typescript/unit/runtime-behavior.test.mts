import type { RenderFrame } from "@fleximark/contracts/protocol";
import { FlexiMarkAdapter } from "fleximark/src/adapter";
import { isPreviewHostMessage } from "fleximark/webview/bridge";
import * as assert from "node:assert/strict";
import { mkdtemp, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { pathToFileURL } from "node:url";

import { createFakeDaemon } from "../support/fake-daemon.mjs";
import { pollUntil, until } from "../support/polling.mjs";
import { setupRuntimeVscode } from "../support/vscode-harness.mjs";
import {
  type RuntimePanel,
  Uri,
  __test,
} from "../support/vscode-pure-stub.mjs";

export const suiteName = "Adapter public runtime behavior";

function frames(panel: RuntimePanel): RenderFrame[] {
  return panel.messages.flatMap((message) =>
    isPreviewHostMessage(message) && message.type === "previewFrame"
      ? [message.frame]
      : [],
  );
}

async function withRuntime(
  run: (runtime: {
    adapter: FlexiMarkAdapter;
    vscode: ReturnType<typeof setupRuntimeVscode>;
    daemon: Awaited<ReturnType<typeof createFakeDaemon>>;
    root: string;
  }) => Promise<void>,
): Promise<void> {
  const root = await mkdtemp(join(tmpdir(), "fleximark-behavior-"));
  const daemon = await createFakeDaemon(root);
  const vscode = setupRuntimeVscode(root);
  const adapter = new FlexiMarkAdapter(vscode.context);
  try {
    await run({ adapter, vscode, daemon, root });
  } finally {
    adapter.dispose();
    for (const instance of await daemon.instances())
      await daemon.crash(instance.pid).catch(() => undefined);
    vscode.reset();
    assert.ok(
      resolve(root).startsWith(resolve(tmpdir(), "fleximark-behavior-")),
    );
    await rm(root, {
      recursive: true,
      force: true,
      maxRetries: 5,
      retryDelay: 50,
    });
  }
}

export function suite(): void {
  test("opens, pushes unsaved updates, closes and changes preview targets", async () => {
    await withRuntime(async ({ adapter, vscode, daemon }) => {
      await adapter.openPreview("embeddedHtml");
      const panel = vscode.panels[0];
      assert.ok(panel);
      panel.ready();
      await until(() => frames(panel).length === 1, "initial embedded frame");
      const first = frames(panel)[0];
      assert.equal(first.documentVersion, 1);
      assert.match(first.blocks[0].html, /Example/);
      vscode.document.update("# Unsaved update\n");
      adapter.changeDocument(vscode.document);
      await until(
        () => frames(panel).some((frame) => frame.documentVersion === 2),
        "pushed unsaved frame",
      );
      const updated = frames(panel).at(-1);
      assert.ok(updated);
      assert.ok(updated.renderRevision > first.renderRevision);
      assert.match(updated.blocks[0].html, /Unsaved update/);
      await adapter.openPreview("externalBrowser");
      assert.equal(vscode.externalUrls.length, 1);
      assert.match(vscode.externalUrls[0], /^http:\/\/127\.0\.0\.1:/);
      assert.equal(vscode.panels.length, 1);
      panel.dispose();
      await until(
        async () =>
          (await daemon.snapshot()).previews.every(
            ({ id }) => id !== first.previewSessionId,
          ),
        "remote embedded preview disposal",
      );
      adapter.closeDocument(vscode.document);
      await until(
        async () => (await daemon.snapshot()).documents.length === 0,
        "document close notification",
      );
      const messages = await daemon.messages();
      assert.equal(
        messages.filter(
          ({ direction, message }) =>
            direction === "received" &&
            message.method === "textDocument/didChange",
        ).length,
        1,
      );
      assert.equal(vscode.errors.length, 0);
    });
  });

  test("reconnects after a daemon disconnect and replays the latest document and preview", async () => {
    await withRuntime(async ({ adapter, vscode, daemon }) => {
      await adapter.openPreview("embeddedHtml");
      const panel = vscode.panels[0];
      panel.ready();
      await until(() => frames(panel).length === 1, "frame before disconnect");
      const first = frames(panel)[0];
      const original = (await daemon.instances())[0];
      vscode.document.update("# Latest text survives reconnection\n");
      adapter.changeDocument(vscode.document);
      await until(
        () => frames(panel).some((frame) => frame.documentVersion === 2),
        "frame before crash",
      );
      await daemon.crash(original.pid);
      await until(
        () =>
          frames(panel).some(
            (frame) => frame.previewSessionId !== first.previewSessionId,
          ),
        "replacement preview frame",
      );
      const recovered = frames(panel).at(-1);
      assert.ok(recovered);
      assert.match(recovered.blocks[0].html, /Latest text survives/);
      assert.equal(recovered.documentVersion, 2);
      assert.equal(vscode.panels.length, 1);
      const current = (await daemon.instances()).at(-1);
      assert.ok(current);
      assert.notEqual(current.daemonInstanceId, original.daemonInstanceId);
      const replay = (await daemon.messages()).filter(
        ({ pid, direction }) => pid === current.pid && direction === "received",
      );
      assert.ok(
        replay.some(({ message }) => message.method === "textDocument/didOpen"),
      );
      assert.ok(
        replay.some(
          ({ message }) => message.method === "fleximark/createPreview",
        ),
      );
    });
  });

  test("invalidates a delayed response when its connection dies and rejects old daemon events", async () => {
    await withRuntime(async ({ adapter, vscode, daemon }) => {
      await adapter.activateDocument(vscode.document);
      const original = (await daemon.instances())[0];
      await daemon.hold("fleximark/createPreview", original.pid);
      const pending = adapter.openPreview("embeddedHtml").then(
        () => undefined,
        (error: unknown) => error,
      );
      await until(
        async () =>
          (await daemon.messages()).some(
            ({ pid, direction, message }) =>
              pid === original.pid &&
              direction === "received" &&
              message.method === "fleximark/createPreview",
          ),
        "delayed old preview request",
      );
      await daemon.crash(original.pid);
      assert.ok((await pending) instanceof Error);
      assert.equal(vscode.panels.length, 0);
      await pollUntil(
        () => daemon.instances(),
        (instances) => instances.length === 2,
        "new daemon connection",
      );
      await adapter.openPreview("embeddedHtml");
      const panel = vscode.panels[0];
      panel.ready();
      await until(() => frames(panel).length === 1, "new connection frame");
      const current = frames(panel)[0];
      await daemon.send({
        jsonrpc: "2.0",
        method: "fleximark/previewChanged",
        params: {
          daemonInstanceId: original.daemonInstanceId,
          previewSessionId: current.previewSessionId,
          renderRevision: current.renderRevision + 1,
          frame: {
            ...current,
            renderRevision: current.renderRevision + 1,
            blocks: [
              {
                id: "node",
                nodeIds: ["node"],
                html: '<p data-fleximark-node-id="node">obsolete daemon</p>',
              },
            ],
          },
        },
      });
      // A request/response round trip is the barrier after the preceding notification.
      await adapter.requestLanguageFeature(
        "textDocument/hover",
        vscode.document,
        {},
      );
      assert.deepEqual(frames(panel), [current]);
      assert.equal(vscode.panels.length, 1);
    });
  });

  test("forwards authenticated embedded navigation and browser source navigation", async () => {
    await withRuntime(async ({ adapter, vscode, daemon }) => {
      await adapter.openPreview("embeddedHtml");
      const panel = vscode.panels[0];
      panel.ready();
      await until(() => frames(panel).length === 1, "navigation frame");
      const frame = frames(panel)[0];
      panel.sendNavigation({
        type: "selectNode",
        previewSessionId: frame.previewSessionId,
        renderRevision: frame.renderRevision,
        nodeId: "node",
      });
      await until(
        async () =>
          (await daemon.messages()).some(
            ({ direction, message }) =>
              direction === "received" &&
              message.method === "fleximark/previewEvent",
          ),
        "embedded navigation forwarding",
      );
      await adapter.openPreview("externalBrowser");
      const preview = (await daemon.snapshot()).previews.find(
        ({ target }) => target === "externalBrowser",
      );
      assert.ok(preview);
      const instance = (await daemon.instances()).at(-1);
      assert.ok(instance);
      await daemon.send({
        jsonrpc: "2.0",
        method: "fleximark/previewEvent",
        params: {
          daemonInstanceId: instance.daemonInstanceId,
          previewSessionId: preview.id,
          renderRevision: preview.frame.renderRevision,
          event: {
            type: "selectSource",
            sourceRange: preview.frame.navigation[0].sourceRange,
          },
        },
      });
      await until(
        () => vscode.editor.reveals.length === 1,
        "browser source reveal",
      );
      assert.equal(vscode.editor.selection.start.line, 0);
      assert.equal(vscode.editor.selection.end.character, 1);
    });
  });

  test("updates workspace membership while keeping the document preview usable", async () => {
    await withRuntime(async ({ adapter, vscode, daemon, root }) => {
      await adapter.openPreview("embeddedHtml");
      const panel = vscode.panels[0];
      panel.ready();
      await until(() => frames(panel).length === 1, "workspace frame");
      const secondRoot = join(root, "second");
      const originalInstance = (await daemon.instances())[0];
      await daemon.install(secondRoot);
      const second = vscode.addWorkspace(secondRoot);
      await adapter.start(second);
      assert.equal((await daemon.instances()).length, 1);
      const hasAddedFolder = () =>
        daemon
          .messages()
          .then((messages) =>
            messages.some(
              ({ direction, message }) =>
                direction === "received" &&
                JSON.stringify(message.params).includes(second.uri.toString()),
            ),
          );
      await until(hasAddedFolder, "workspace addition reaches daemon");
      const removalStart = (await daemon.messages()).length;
      vscode.removeWorkspace(second.uri.toString());
      adapter.removeWorkspace(second);
      await adapter.activateDocument(vscode.document);
      vscode.document.update("# Preview after workspace removal\n");
      adapter.changeDocument(vscode.document);
      await until(
        () =>
          frames(panel).some(
            (frame) =>
              frame.documentVersion === 2 &&
              frame.blocks[0].html.includes("after workspace removal"),
          ),
        "preview after folder removal",
      );
      const messages = await daemon.messages();
      assert.ok(
        messages
          .slice(removalStart)
          .some(
            ({ direction, message }) =>
              direction === "received" &&
              ((message.method === "workspace/didChangeWorkspaceFolders" &&
                JSON.stringify(message.params).includes(
                  second.uri.toString(),
                )) ||
                (message.method === "initialize" &&
                  !JSON.stringify(message.params).includes(
                    second.uri.toString(),
                  ))),
          ),
      );
      assert.equal(vscode.panels.length, 1);
      assert.equal(
        (await daemon.instances()).at(-1)?.daemonInstanceId,
        originalInstance.daemonInstanceId,
      );
      assert.ok(
        messages.some(
          ({ direction, message }) =>
            direction === "received" &&
            message.method === "fleximark/reconfigureWorkspace" &&
            message.params?.trusted === true &&
            message.params.workspaceUri === second.uri.toString(),
        ),
      );
      const loose = {
        ...vscode.document,
        uri: new Uri(
          pathToFileURL(join(root, "..", "loose.md")).href,
        ) as typeof vscode.document.uri,
        getText: () => "# Loose Markdown\n",
      };
      const looseEditor = { ...vscode.editor, document: loose };
      __test.textDocuments.push(loose);
      __test.visibleTextEditors.push(looseEditor);
      __test.activeTextEditor = looseEditor;
      await adapter.activateEditor(looseEditor);
      assert.equal(
        (await daemon.instances()).length,
        1,
        "detached Markdown keeps the live daemon",
      );
    });
  });
}
