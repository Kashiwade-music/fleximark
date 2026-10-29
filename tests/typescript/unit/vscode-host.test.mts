import { previewRuntimes } from "@fleximark/preview/runtimes";
import * as assert from "node:assert/strict";

import { installDomGlobals, previewDom } from "../support/dom-fixtures.mjs";
import { specialFrame } from "../support/preview-fixtures.mjs";

export const suiteName = "VS Code preview host";

export function suite(): void {
  test("authenticates and pins the host when VS Code hides window.parent before startup", async () => {
    const { window } = previewDom(
      '<!doctype html><html><head><meta name="fleximark-message-token" content="token"></head><body><main id="preview"></main></body></html>',
    );
    let globals: ReturnType<typeof installDomGlobals> | undefined;
    const messages: unknown[] = [];
    const hostWindow = {};
    const frame = specialFrame(
      "mermaid",
      "graph TD; A--&gt;B",
      { navigation: [] },
      "mermaid",
    );
    const originalRender = previewRuntimes.mermaid.render;
    try {
      Object.defineProperty(window, "location", {
        configurable: true,
        value: { origin: "vscode-webview://test" },
      });
      Object.defineProperty(window, "parent", {
        configurable: true,
        writable: true,
        value: window,
      });
      previewRuntimes.mermaid.render = async () => ({ svg: "<svg></svg>" });
      globals = installDomGlobals(window, {
        acquireVsCodeApi: {
          configurable: true,
          value: () => ({
            postMessage: (message: unknown) => messages.push(message),
          }),
        },
      });
      await import("fleximark/webview/vscode-host");
      assert.deepEqual(messages, [{ type: "ready" }]);
      const event = new window.Event("message");
      Object.defineProperties(event, {
        origin: { value: window.location.origin },
        source: { value: hostWindow },
      });
      Object.defineProperty(event, "data", {
        value: {
          type: "previewFrame",
          messageToken: "token",
          frame,
        },
      });
      window.dispatchEvent(event);
      await Promise.resolve();
      await Promise.resolve();
      assert.ok(window.document.querySelector("svg"));
      assert.deepEqual(messages, [{ type: "ready" }]);

      const invalid = new window.Event("message");
      Object.defineProperties(invalid, {
        origin: { value: window.location.origin },
        source: { value: hostWindow },
      });
      Object.defineProperty(invalid, "data", {
        value: {
          type: "previewFrame",
          messageToken: "token",
          frame: {
            ...frame,
            blocks: "invalid",
          },
        },
      });
      for (const [origin, source, token] of [
        [window.location.origin, {}, "token"],
        ["https://untrusted.example", hostWindow, "token"],
        [window.location.origin, hostWindow, "wrong-token"],
      ]) {
        const foreign = new window.Event("message");
        Object.defineProperties(foreign, {
          origin: { value: origin },
          source: { value: source },
          data: {
            value: {
              type: "previewFrame",
              messageToken: token,
              frame: {
                ...frame,
                blocks: "invalid",
              },
            },
          },
        });
        window.dispatchEvent(foreign);
        assert.deepEqual(messages, [{ type: "ready" }]);
        assert.ok(window.document.querySelector("svg"));
      }
      window.dispatchEvent(invalid);
      assert.ok(window.document.querySelector("svg"));
      window.dispatchEvent(invalid);
      assert.deepEqual(messages, [{ type: "ready" }, { type: "requestFrame" }]);
      assert.ok(window.document.querySelector("svg"));
      assert.equal(
        window.document
          .querySelector("#preview")
          ?.getAttribute("data-fleximark-error"),
        "Preview unavailable. Reopen it to retry.",
      );

      window.dispatchEvent(event);
      assert.ok(window.document.querySelector("svg"));
      assert.deepEqual(messages, [{ type: "ready" }, { type: "requestFrame" }]);
    } finally {
      previewRuntimes.mermaid.render = originalRender;
      globals?.dispose();
    }
  });
}
