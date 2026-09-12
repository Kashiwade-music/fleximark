import { parseHTML } from "linkedom";
import * as assert from "node:assert/strict";

import { previewRuntimes } from "../web/preview-client/runtimes.mjs";

export const suiteName = "VS Code preview host";

export function suite(): void {
  test("acknowledges a Mermaid snapshot before asynchronous enhancement completes", async () => {
    const names = [
      "document",
      "HTMLElement",
      "HTMLScriptElement",
      "Node",
      "Element",
      "Event",
      "window",
      "acquireVsCodeApi",
    ] as const;
    const descriptors = new Map(
      names.map((name) => [
        name,
        Object.getOwnPropertyDescriptor(globalThis, name),
      ]),
    );
    const { window } = parseHTML(
      '<!doctype html><html><head><meta name="fleximark-message-token" content="token"></head><body><main id="preview"></main></body></html>',
    );
    const messages: unknown[] = [];
    const originalMermaidRender = previewRuntimes.mermaid.render;
    let mermaidRenderCount = 0;
    let resolveMermaid!: (value: { svg: string }) => void;
    const mermaidRender = new Promise<{ svg: string }>((resolve) => {
      resolveMermaid = resolve;
    });
    const dispatchMessage = (data: unknown) => {
      const event = new window.Event("message");
      Object.defineProperty(event, "data", { value: data });
      window.dispatchEvent(event);
    };

    try {
      previewRuntimes.mermaid.render = () => {
        mermaidRenderCount += 1;
        return mermaidRender;
      };
      Object.defineProperties(globalThis, {
        document: { configurable: true, value: window.document },
        HTMLElement: { configurable: true, value: window.HTMLElement },
        HTMLScriptElement: {
          configurable: true,
          value: window.HTMLScriptElement,
        },
        Node: { configurable: true, value: window.Node },
        Element: { configurable: true, value: window.Element },
        Event: { configurable: true, value: window.Event },
        window: { configurable: true, value: window },
        acquireVsCodeApi: {
          configurable: true,
          value: () => ({
            postMessage: (message: unknown) => messages.push(message),
          }),
        },
      });

      await import("../web/preview-client/vscode-host.mjs");
      assert.deepEqual(messages, [{ type: "ready" }]);

      dispatchMessage({
        type: "initializePreview",
        messageToken: "token",
        publication: {
          ...snapshot(),
          previewSessionId: "mermaid-preview",
          resultRenderRevision: 8,
          nodeIds: ["document-root", "mermaid"],
          navigation: [
            {
              ...snapshot().navigation[0],
              nodeId: "mermaid",
            },
          ],
          html: '<main data-fleximark-node-id="document-root"><div data-fleximark-node-id="mermaid" data-fleximark-kind="mermaid"><script type="application/json">"graph TD; A--&gt;B"</script></div></main>',
        },
      });
      assert.deepEqual(messages.at(-1), {
        type: "rendered",
        previewSessionId: "mermaid-preview",
        renderRevision: 8,
      });
      assert.equal(mermaidRenderCount, 1);
      assert.equal(window.document.querySelector("svg"), null);
      const acknowledgedMessageCount = messages.length;
      resolveMermaid({ svg: "<svg></svg>" });
      await mermaidRender;
      await Promise.resolve();
      await Promise.resolve();
      assert.equal(messages.length, acknowledgedMessageCount);
      assert.ok(window.document.querySelector("svg"));

      window.dispatchEvent(new window.Event("unload"));
      assert.equal(
        window.document.querySelectorAll("style[data-fleximark-theme]").length,
        0,
      );
    } finally {
      previewRuntimes.mermaid.render = originalMermaidRender;
      for (const name of names) {
        const descriptor = descriptors.get(name);
        if (descriptor) Object.defineProperty(globalThis, name, descriptor);
        else Reflect.deleteProperty(globalThis, name);
      }
    }
  });
}

function snapshot() {
  return {
    type: "full" as const,
    previewSessionId: "preview",
    documentVersion: 3,
    resultRenderRevision: 7,
    rendererFingerprint: "sha256:renderer",
    nodeIds: ["document-root", "paragraph"],
    navigation: [
      {
        nodeId: "paragraph",
        sourceRange: {
          byteStart: 0,
          byteEnd: 5,
          start: { line: 0, character: 0, encoding: "utf8" as const },
          end: { line: 0, character: 5, encoding: "utf8" as const },
        },
        depth: 0,
      },
    ],
    style: null,
    assets: [],
    html: '<main data-fleximark-node-id="document-root"><p data-fleximark-node-id="paragraph">ready</p></main>',
  };
}
