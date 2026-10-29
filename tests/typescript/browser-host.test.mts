import { parseHTML } from "linkedom";
import * as assert from "node:assert/strict";

import { BrowserNavigationTransport } from "../../web/preview/browser-navigation.mjs";

export const suiteName = "Browser preview host";

export function suite(): void {
  test("calls browser fetch with its global receiver", () => {
    const sent: unknown[] = [];
    const fetcher = function (
      this: unknown,
      input: RequestInfo | URL,
      init?: RequestInit,
    ) {
      assert.equal(this, globalThis);
      assert.equal(input, "/preview/navigation");
      sent.push(JSON.parse(init?.body as string));
      return Promise.resolve({ ok: true } as Response);
    } as typeof fetch;
    const navigation = new BrowserNavigationTransport(
      "/preview/navigation",
      fetcher,
    );
    const event = {
      type: "revealNode" as const,
      nodeId: "visible-node",
      previewSessionId: "preview",
      renderRevision: 1,
    };
    navigation.send(event);
    assert.deepEqual(sent, [event]);
    navigation.dispose();
  });

  test("applies pushed frames and ordered navigation without frame fetches", async () => {
    const names = [
      "document",
      "HTMLElement",
      "HTMLScriptElement",
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
    const fetches: string[] = [];
    const navigationPosts: unknown[] = [];
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
        HTMLScriptElement: {
          configurable: true,
          value: window.HTMLScriptElement,
        },
        Node: { configurable: true, value: window.Node },
        Element: { configurable: true, value: window.Element },
        Event: { configurable: true, value: window.Event },
        window: { configurable: true, value: window },
        EventSource: { configurable: true, value: FakeEventSource },
        fetch: {
          configurable: true,
          value: (input: string, init?: RequestInit) => {
            if (init?.method === "POST") {
              assert.equal(input, "/preview/navigation");
              navigationPosts.push(JSON.parse(init.body as string));
              return Promise.resolve({ ok: true } as Response);
            }
            fetches.push(input);
            assert.fail("frames must arrive over SSE");
          },
        },
        location: { configurable: true, value: { pathname: "/preview" } },
      });

      await import("../../web/preview/browser-host.mjs");
      const send = (value: unknown) =>
        messageHandler?.({
          data: JSON.stringify(value),
        } as MessageEvent<string>);
      send(frame(1, "one"));
      assert.equal(
        window.document.querySelector("#preview")?.textContent,
        "one",
      );
      send({
        type: "selection",
        previewSessionId: "preview",
        renderRevision: 1,
        nodeIds: ["paragraph"],
        activePosition: null,
      });
      assert.equal(
        window.document
          .querySelector("p")
          ?.getAttribute("data-fleximark-selected"),
        "true",
      );
      // A navigation arriving before its frame is retained until that frame.
      send({
        type: "selection",
        previewSessionId: "preview",
        renderRevision: 2,
        nodeIds: ["paragraph"],
        activePosition: null,
      });
      send(frame(2, "two"));
      assert.equal(
        window.document.querySelector("#preview")?.textContent,
        "two",
      );
      assert.equal(
        window.document
          .querySelector("p")
          ?.getAttribute("data-fleximark-selected"),
        "true",
      );
      assert.deepEqual(fetches, []);
      const paragraph = window.document.querySelector("p");
      assert.ok(paragraph);
      paragraph.getBoundingClientRect = () =>
        ({ top: 0, bottom: 20, height: 20 }) as DOMRect;
      Object.defineProperty(window, "innerHeight", {
        value: 720,
        configurable: true,
      });
      window.dispatchEvent(new window.Event("scroll"));
      await new Promise((resolve) => setTimeout(resolve, 120));
      assert.deepEqual(navigationPosts, [
        {
          type: "revealNode",
          nodeId: "paragraph",
          previewSessionId: "preview",
          renderRevision: 2,
        },
      ]);
      window.dispatchEvent(new window.Event("unload"));
      assert.equal(closeCount, 1);
    } finally {
      for (const name of names) {
        const descriptor = descriptors.get(name);
        if (descriptor) Object.defineProperty(globalThis, name, descriptor);
        else Reflect.deleteProperty(globalThis, name);
      }
    }
  });
}

function frame(renderRevision: number, text: string) {
  return {
    previewSessionId: "preview",
    documentVersion: renderRevision,
    renderRevision,
    rendererFingerprint: "a".repeat(64),
    style: null,
    assets: [],
    blocks: [
      {
        id: "paragraph",
        nodeIds: ["paragraph"],
        html: `<p data-fleximark-node-id="paragraph">${text}</p>`,
      },
    ],
    navigation: [
      {
        nodeId: "paragraph",
        depth: 0,
        sourceRange: {
          byteStart: 0,
          byteEnd: text.length,
          start: { line: 0, character: 0, encoding: "utf8" },
          end: { line: 0, character: text.length, encoding: "utf8" },
        },
      },
    ],
  };
}
