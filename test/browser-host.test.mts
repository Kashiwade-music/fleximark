import { parseHTML } from "linkedom";
import * as assert from "node:assert/strict";

export const suiteName = "Browser preview host";

export function suite(): void {
  test("observes live navigation failures and closes broken SSE once", async () => {
    const names = [
      "document",
      "HTMLElement",
      "Node",
      "Element",
      "Event",
      "window",
      "EventSource",
      "fetch",
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
    let nonOkResponsesObserved = 0;
    const registeredEventTypes: string[] = [];
    const fetches: { input: string; init?: RequestInit }[] = [];
    class FakeEventSource {
      constructor(readonly url: string) {}

      addEventListener(
        type: string,
        listener: (event: MessageEvent<string>) => void,
      ): void {
        registeredEventTypes.push(type);
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
        fetch: {
          configurable: true,
          value: (input: string, init?: RequestInit) => {
            fetches.push({ input, init });
            if (fetches.length === 1)
              return Promise.resolve({
                get ok() {
                  nonOkResponsesObserved += 1;
                  return false;
                },
              } as Response);
            if (fetches.length === 2)
              return Promise.reject(new Error("navigation failed"));
            if (fetches.length === 3)
              throw new Error("navigation failed synchronously");
            return new Promise<Response>(() => undefined);
          },
        },
        location: {
          configurable: true,
          value: { pathname: "/preview", reload: () => (reloadCount += 1) },
        },
      });

      await import("../web/preview-client/browser-host.mjs");
      assert.ok(messageHandler);
      // EventSource errors remain native reconnect signals, not fatal preview events.
      assert.deepEqual(registeredEventTypes, ["message"]);
      assert.equal(closeCount, 0);
      assert.equal(reloadCount, 0);
      messageHandler?.({
        data: JSON.stringify([
          {
            type: "full",
            previewSessionId: "preview",
            documentVersion: 1,
            resultRenderRevision: 1,
            rendererFingerprint: "sha256:renderer",
            nodeIds: ["document-root", "paragraph"],
            navigation: [
              {
                nodeId: "paragraph",
                sourceRange: {
                  byteStart: 0,
                  byteEnd: 3,
                  start: { line: 0, character: 0, encoding: "utf8" },
                  end: { line: 0, character: 3, encoding: "utf8" },
                },
                depth: 0,
              },
            ],
            style: null,
            assets: [],
            html: '<main data-fleximark-node-id="document-root"><p data-fleximark-node-id="paragraph">ready</p></main>',
          },
        ]),
      } as MessageEvent<string>);
      window.document
        .querySelector("[data-fleximark-node-id=paragraph]")
        ?.dispatchEvent(new window.Event("click", { bubbles: true }));
      window.document
        .querySelector("[data-fleximark-node-id=paragraph]")
        ?.dispatchEvent(new window.Event("click", { bubbles: true }));
      window.document
        .querySelector("[data-fleximark-node-id=paragraph]")
        ?.dispatchEvent(new window.Event("click", { bubbles: true }));
      window.document
        .querySelector("[data-fleximark-node-id=paragraph]")
        ?.dispatchEvent(new window.Event("click", { bubbles: true }));
      await Promise.resolve();
      await Promise.resolve();
      assert.equal(fetches.length, 4);
      for (const { input, init } of fetches) {
        assert.equal(input, "/preview/navigation");
        assert.equal(init?.method, "POST");
        assert.deepEqual(init?.headers, {
          "content-type": "application/json",
        });
        assert.equal(
          init?.body,
          JSON.stringify({
            type: "selectNode",
            nodeId: "paragraph",
            previewSessionId: "preview",
            renderRevision: 1,
          }),
        );
        assert.equal(init?.credentials, "same-origin");
        assert.ok(init?.signal);
        assert.equal(init.signal.aborted, false);
      }
      assert.equal(nonOkResponsesObserved, 1);
      assert.equal(closeCount, 0);
      assert.equal(reloadCount, 0);
      const before = window.document.querySelector("#preview")?.innerHTML;
      window.dispatchEvent(new window.Event("unload"));
      assert.equal(closeCount, 1);
      assert.equal(reloadCount, 0);
      assert.ok(fetches.every(({ init }) => init?.signal?.aborted));
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
      assert.equal(closeCount, 2);
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
