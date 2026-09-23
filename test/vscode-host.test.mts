import { parseHTML } from "linkedom";
import * as assert from "node:assert/strict";

import { previewRuntimes } from "../web/preview-client/runtimes.mjs";

export const suiteName = "VS Code preview host";

export function suite(): void {
  test("applies a frame without a rendered acknowledgement", async () => {
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
    const originalRender = previewRuntimes.mermaid.render;
    try {
      previewRuntimes.mermaid.render = async () => ({ svg: "<svg></svg>" });
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
      const event = new window.Event("message");
      Object.defineProperty(event, "data", {
        value: {
          type: "previewFrame",
          messageToken: "token",
          frame: frame(),
        },
      });
      window.dispatchEvent(event);
      await Promise.resolve();
      await Promise.resolve();
      assert.ok(window.document.querySelector("svg"));
      assert.deepEqual(messages, [{ type: "ready" }]);

      const invalid = new window.Event("message");
      Object.defineProperty(invalid, "data", {
        value: {
          type: "previewFrame",
          messageToken: "token",
          frame: { ...frame(), blocks: "invalid" },
        },
      });
      window.dispatchEvent(invalid);
      assert.ok(window.document.querySelector("svg"));
      window.dispatchEvent(invalid);
      assert.deepEqual(messages, [{ type: "ready" }, { type: "requestFrame" }]);
      assert.equal(window.document.querySelector("svg"), null);
      assert.equal(
        window.document.querySelector("#preview")?.textContent,
        "Preview unavailable. Reopen it to retry.",
      );

      window.dispatchEvent(event);
      assert.equal(window.document.querySelector("svg"), null);
      assert.deepEqual(messages, [{ type: "ready" }, { type: "requestFrame" }]);
    } finally {
      previewRuntimes.mermaid.render = originalRender;
      for (const name of names) {
        const descriptor = descriptors.get(name);
        if (descriptor) Object.defineProperty(globalThis, name, descriptor);
        else Reflect.deleteProperty(globalThis, name);
      }
    }
  });
}

function frame() {
  return {
    previewSessionId: "preview",
    documentVersion: 1,
    renderRevision: 1,
    rendererFingerprint: "a".repeat(64),
    style: null,
    assets: [],
    blocks: [
      {
        id: "mermaid",
        nodeIds: ["mermaid"],
        html: '<div data-fleximark-node-id="mermaid" data-fleximark-kind="mermaid"><script type="application/json">"graph TD; A--&gt;B"</script></div>',
      },
    ],
    navigation: [],
    annotations: {},
  };
}
