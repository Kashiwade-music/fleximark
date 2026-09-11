import { parseHTML } from "linkedom";
import * as assert from "node:assert/strict";

export const suiteName = "Browser preview host";

export function suite(): void {
  test("broken SSE closes the stream and reloads only once", async () => {
    const names = [
      "document",
      "HTMLElement",
      "Node",
      "Element",
      "Event",
      "window",
      "EventSource",
      "location",
    ] as const;
    const descriptors = new Map(
      names.map((name) => [
        name,
        Object.getOwnPropertyDescriptor(globalThis, name),
      ]),
    );
    const { window } = parseHTML(
      "<!doctype html><html><body><main id=preview></main></body></html>",
    );
    let messageHandler: ((event: MessageEvent<string>) => void) | undefined;
    let closeCount = 0;
    let reloadCount = 0;
    class FakeEventSource {
      constructor(readonly url: string) {}

      addEventListener(
        type: string,
        listener: (event: MessageEvent<string>) => void,
      ): void {
        if (type === "message") messageHandler = listener;
      }

      close(): void {
        closeCount += 1;
      }
    }

    try {
      Object.defineProperty(window.document, "currentScript", {
        configurable: true,
        value: {
          hasAttribute: (name: string) => name === "data-fleximark-live",
        },
      });
      Object.defineProperties(globalThis, {
        document: { configurable: true, value: window.document },
        HTMLElement: { configurable: true, value: window.HTMLElement },
        Node: { configurable: true, value: window.Node },
        Element: { configurable: true, value: window.Element },
        Event: { configurable: true, value: window.Event },
        window: { configurable: true, value: window },
        EventSource: { configurable: true, value: FakeEventSource },
        location: {
          configurable: true,
          value: { pathname: "/preview", reload: () => (reloadCount += 1) },
        },
      });

      await import("../web/preview-client/browser-host.mjs");
      assert.ok(messageHandler);
      const before = window.document.querySelector("#preview")?.innerHTML;
      assert.doesNotThrow(() =>
        messageHandler?.({ data: "{" } as MessageEvent<string>),
      );
      assert.doesNotThrow(() =>
        messageHandler?.({ data: "not json" } as MessageEvent<string>),
      );
      assert.doesNotThrow(() =>
        messageHandler?.({
          data: JSON.stringify([
            {
              type: "full",
              previewSessionId: "preview",
              documentVersion: 1,
              resultRenderRevision: 1,
              rendererFingerprint: "sha256:renderer",
              nodeIds: ["document-root"],
              navigation: [],
              style: null,
              assets: [],
              html: '<main data-fleximark-node-id="document-root">queued</main>',
            },
          ]),
        } as MessageEvent<string>),
      );
      window.dispatchEvent(new window.Event("unload"));
      assert.equal(closeCount, 1);
      assert.equal(reloadCount, 1);
      assert.equal(
        window.document.querySelector("#preview")?.innerHTML,
        before,
      );
      assert.equal(
        window.document.querySelectorAll("style[data-fleximark-theme]").length,
        0,
      );
    } finally {
      for (const name of names) {
        const descriptor = descriptors.get(name);
        if (descriptor) Object.defineProperty(globalThis, name, descriptor);
        else Reflect.deleteProperty(globalThis, name);
      }
    }
  });
}
