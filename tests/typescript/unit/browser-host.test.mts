import { BrowserNavigationTransport } from "@fleximark/preview/browser-navigation";
import { PreviewSession } from "@fleximark/preview/session";
import * as assert from "node:assert/strict";

import { installDomGlobals, previewDom } from "../support/dom-fixtures.mjs";
import { textFrame as frame } from "../support/preview-fixtures.mjs";

export const suiteName = "Browser preview host";

export function suite(): void {
  test("orders future navigation and bounds recovery until a successful frame", async () => {
    const applied: number[] = [];
    const navigated: number[] = [];
    let recoveries = 0;
    let errors = 0;
    let accept = true;
    const session = new PreviewSession({
      apply: (value) => {
        if (accept) applied.push(value.renderRevision);
        return accept;
      },
      navigate: (value) => {
        navigated.push(value.renderRevision);
      },
      requestRecovery: () => {
        recoveries++;
      },
      reportError: () => {
        errors++;
      },
    });
    const selection = (revision: number) => ({
      type: "selection" as const,
      previewSessionId: "preview",
      renderRevision: revision,
      nodeIds: ["paragraph"],
      activePosition: null,
    });
    await session.receiveNavigation(selection(2));
    await session.receiveFrame(frame(1, "one"));
    assert.deepEqual(navigated, []);
    await session.receiveFrame(frame(2, "two"));
    await session.receiveNavigation(selection(1));
    assert.deepEqual(navigated, [2]);
    accept = false;
    await session.receiveFrame(frame(3, "bad"));
    await session.receiveFrame(frame(4, "bad"));
    assert.equal(session.renderRevision, 2);
    assert.equal(recoveries, 1);
    assert.equal(errors, 1);
    accept = true;
    await session.receiveFrame(frame(5, "five"));
    session.fail(new Error("next burst"));
    assert.equal(recoveries, 2);
    assert.deepEqual(applied, [1, 2, 5]);
    session.dispose();
    await session.receiveFrame(frame(6, "disposed"));
    assert.equal(session.renderRevision, 5);
  });

  test("drains navigation only after an asynchronous frame is accepted", async () => {
    const calls: string[] = [];
    let release: ((accepted: boolean) => void) | undefined;
    const session = new PreviewSession({
      apply: () => {
        calls.push("apply");
        return new Promise<boolean>((resolve) => {
          release = resolve;
        });
      },
      navigate: () => {
        calls.push("navigate");
      },
      requestRecovery: () => assert.fail("unexpected recovery"),
      reportError: () => assert.fail("unexpected failure"),
    });
    const framePending = session.receiveFrame(frame(1, "one"));
    const navigationPending = session.receiveNavigation({
      type: "selection",
      previewSessionId: "preview",
      renderRevision: 1,
      nodeIds: ["paragraph"],
      activePosition: null,
    });
    assert.deepEqual(calls, ["apply"]);
    assert.ok(release);
    release(true);
    await framePending;
    await navigationPending;
    assert.deepEqual(calls, ["apply", "navigate"]);
  });

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
    const { window } = previewDom(
      "<!doctype html><html><body><main id=preview></main></body></html>",
    );
    let globals: ReturnType<typeof installDomGlobals> | undefined;
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
      globals = installDomGlobals(window, {
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

      await import("@fleximark/preview/browser-host");
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
      globals?.dispose();
    }
  });
}
