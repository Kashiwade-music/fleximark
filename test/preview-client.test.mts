import { parseHTML } from "linkedom";
import * as assert from "node:assert/strict";

import {
  PreviewEnhancer,
  type PreviewRuntimes,
} from "../web/preview-client/enhance.mjs";
import {
  PreviewHost,
  isPreviewHostMessage,
  isPreviewHostMessageEvent,
} from "../web/preview-client/host.mjs";
import {
  PreviewDocument,
  type RenderPatch,
  type RenderSnapshot,
} from "../web/preview-client/index.mjs";
import { PreviewNavigation } from "../web/preview-client/navigation.mjs";

export const suiteName = "Preview client";

export function suite(): void {
  let root: HTMLElement;
  let requested: number;
  let preview: PreviewDocument;

  setup(() => {
    const { window } = parseHTML(
      "<!doctype html><html><body><main id=preview></main></body></html>",
    );
    Object.defineProperty(window.document, "compatMode", {
      configurable: true,
      value: "CSS1Compat",
    });
    Object.defineProperties(globalThis, {
      document: { configurable: true, value: window.document },
      HTMLElement: { configurable: true, value: window.HTMLElement },
      Node: { configurable: true, value: window.Node },
      HTMLScriptElement: {
        configurable: true,
        value: window.HTMLScriptElement,
      },
      Element: { configurable: true, value: window.Element },
      Event: { configurable: true, value: window.Event },
      CSSStyleSheet: {
        configurable: true,
        value: class {
          readonly cssRules: unknown[] = [];
          insertRule(rule: unknown): void {
            this.cssRules.push(rule);
          }
          replaceSync(): void {
            this.cssRules.length = 0;
          }
        },
      },
      window: { configurable: true, value: window },
    });
    root = window.document.querySelector("#preview") as unknown as HTMLElement;
    Object.assign(window.Element.prototype, {
      getBBox: () => ({ x: 0, y: 0, width: 100, height: 20 }),
      getComputedTextLength: () => 100,
    });
    requested = 0;
    preview = new PreviewDocument(root, () => requested++);
  });

  teardown(() => {
    preview.dispose();
    Reflect.deleteProperty(globalThis, "document");
    Reflect.deleteProperty(globalThis, "HTMLElement");
    Reflect.deleteProperty(globalThis, "Node");
    Reflect.deleteProperty(globalThis, "HTMLScriptElement");
    Reflect.deleteProperty(globalThis, "Element");
    Reflect.deleteProperty(globalThis, "Event");
    Reflect.deleteProperty(globalThis, "CSSStyleSheet");
    Reflect.deleteProperty(globalThis, "window");
  });

  test("applies a valid single-root snapshot and patch", () => {
    assert.equal(preview.applySnapshot(snapshot()), true);
    assert.match(root.innerHTML, />one</);
    assert.equal(
      preview.applyPatch(
        patch([
          {
            type: "replace",
            nodeId: "a",
            parentId: "document-root",
            contentNodeIds: ["a"],
            content: '<p data-fleximark-node-id="a">two</p>',
            precondition: {
              nodeExists: true,
              currentParentId: "document-root",
            },
          },
        ]),
      ),
      true,
    );
    assert.match(root.innerHTML, />two</);
    assert.equal(requested, 0);
  });

  test("rejects a later invalid operation without committing earlier operations", () => {
    preview.applySnapshot(snapshot());
    const before = root.innerHTML;
    assert.equal(
      preview.applyPatch(
        patch([
          {
            type: "replace",
            nodeId: "a",
            parentId: "document-root",
            contentNodeIds: ["a"],
            content: '<p data-fleximark-node-id="a">changed</p>',
            precondition: {
              nodeExists: true,
              currentParentId: "document-root",
            },
          },
          {
            type: "insert",
            nodeId: "b",
            parentId: "document-root",
            beforeId: null,
            afterId: null,
            atEnd: true,
            contentNodeIds: ["wrong"],
            content: '<p data-fleximark-node-id="b">invalid</p>',
          },
        ]),
      ),
      false,
    );
    assert.equal(root.innerHTML, before);
    assert.equal(requested, 1);
  });

  test("applies allowlisted attributes and atomically rejects dangerous ones", () => {
    const initial = snapshot();
    initial.html =
      '<main data-fleximark-node-id="document-root"><aside data-fleximark-node-id="a" data-admonition-kind="info">one</aside></main>';
    preview.applySnapshot(initial);
    assert.equal(
      preview.applyPatch(
        patch([
          {
            type: "setAttributes",
            nodeId: "a",
            parentId: "document-root",
            attributes: { "data-admonition-kind": "tip" },
            precondition: {
              nodeExists: true,
              currentParentId: "document-root",
            },
          },
        ]),
      ),
      true,
    );
    assert.equal(
      root.querySelector("aside")?.getAttribute("data-admonition-kind"),
      "tip",
    );

    const before = root.innerHTML;
    const unsafe = patch([
      {
        type: "setAttributes",
        nodeId: "a",
        parentId: "document-root",
        attributes: { onclick: "alert(1)", srcdoc: "<script>" },
        precondition: {
          nodeExists: true,
          currentParentId: "document-root",
        },
      },
    ]);
    unsafe.baseRenderRevision = 2;
    unsafe.resultRenderRevision = 3;
    assert.equal(preview.applyPatch(unsafe), false);
    assert.equal(root.innerHTML, before);
    assert.equal(requested, 1);
  });

  test("commits patch DOM and navigation as one atomic revision", () => {
    preview.applySnapshot(snapshot());
    const before = root.innerHTML;
    const invalid = patch([
      {
        type: "replace",
        nodeId: "a",
        parentId: "document-root",
        contentNodeIds: ["a"],
        content: '<p data-fleximark-node-id="a">changed</p>',
        precondition: {
          nodeExists: true,
          currentParentId: "document-root",
        },
      },
    ]);
    invalid.navigation = [
      {
        ...navigation("a", 0),
        sourceRange: {
          ...navigation("a", 0).sourceRange,
          start: { line: 0, character: 1, encoding: "invalid" as "utf8" },
        },
      },
    ];
    assert.equal(preview.applyPatch(invalid), false);
    assert.equal(root.innerHTML, before);
    assert.deepEqual(preview.navigation, snapshot().navigation);
    assert.equal(requested, 1);
  });

  test("ignores a stale full snapshot from the same session atomically", () => {
    const current = {
      ...snapshot(),
      resultRenderRevision: 2,
      style: { css: "p{color:green}", fingerprint: "a".repeat(64) },
    };
    assert.equal(preview.applySnapshot(current), true);
    const beforeDom = root.innerHTML;
    const beforeNavigation = [...preview.navigation];
    const beforeStyle = document.querySelector(
      "style[data-fleximark-theme]",
    )?.textContent;
    assert.equal(
      preview.applySnapshot({
        ...snapshot(),
        html: "<script>stale content must not be parsed</script>",
        navigation: [navigation("a", 99)],
        style: { css: "p{color:red}", fingerprint: "b".repeat(64) },
      }),
      false,
    );
    assert.equal(root.innerHTML, beforeDom);
    assert.deepEqual(preview.navigation, beforeNavigation);
    assert.equal(
      document.querySelector("style[data-fleximark-theme]")?.textContent,
      beforeStyle,
    );
    assert.equal(requested, 0);

    assert.equal(
      preview.applyPatch({
        ...patch([]),
        documentVersion: 3,
        baseRenderRevision: 2,
        resultRenderRevision: 3,
        style: current.style,
      }),
      true,
    );
    assert.equal(
      preview.applySnapshot({
        ...snapshot(),
        previewSessionId: "preview-2",
        html: '<main data-fleximark-node-id="document-root"><p data-fleximark-node-id="a">replacement</p></main>',
      }),
      true,
    );
    assert.match(root.innerHTML, />replacement</);
    assert.equal(requested, 0);
  });

  test("applies theme CSS safely and rejects a style-changing patch", () => {
    const themed = {
      ...snapshot(),
      style: {
        css: "[data-fleximark-node-id=a]{color:rgb(1 2 3)}",
        fingerprint: "a".repeat(64),
      },
    };
    assert.equal(preview.applySnapshot(themed), true);
    assert.equal(
      document.querySelector("style[data-fleximark-theme]")?.textContent,
      themed.style.css,
    );
    const changed = patch([]);
    changed.style = { css: "p{color:red}", fingerprint: "b".repeat(64) };
    assert.equal(preview.applyPatch(changed), false);
    assert.equal(
      document.querySelector("style[data-fleximark-theme]")?.textContent,
      themed.style.css,
    );
    assert.equal(requested, 1);
  });

  test("resolves typed local assets to blob URLs and revokes replaced blobs", () => {
    const created: string[] = [];
    const revoked: string[] = [];
    const originalCreate = URL.createObjectURL;
    const originalRevoke = URL.revokeObjectURL;
    URL.createObjectURL = () => {
      const value = `blob:fleximark-${created.length + 1}`;
      created.push(value);
      return value;
    };
    URL.revokeObjectURL = (value) => revoked.push(value);
    try {
      const first = snapshot();
      first.assets = [asset("aGVsbG8=")];
      first.html =
        '<main data-fleximark-node-id="document-root"><img data-fleximark-node-id="a" src="fleximark-asset:' +
        assetHash +
        '"></main>';
      assert.equal(preview.applySnapshot(first), true);
      assert.equal(root.querySelector("img")?.getAttribute("src"), created[0]);

      const replacement = { ...first, resultRenderRevision: 2 };
      assert.equal(preview.applySnapshot(replacement), true);
      assert.deepEqual(revoked, [created[0]]);
      preview.dispose();
      assert.deepEqual(revoked, created);
    } finally {
      URL.createObjectURL = originalCreate;
      URL.revokeObjectURL = originalRevoke;
    }
  });

  test("resolves an opaque asset reference on top-level patch content", () => {
    const originalCreate = URL.createObjectURL;
    const originalRevoke = URL.revokeObjectURL;
    URL.createObjectURL = () => "blob:fleximark-top-level";
    URL.revokeObjectURL = () => undefined;
    try {
      const first = snapshot();
      first.assets = [asset("aGVsbG8=")];
      assert.equal(preview.applySnapshot(first), true);
      const replacement = patch([
        {
          type: "replace",
          nodeId: "a",
          parentId: "document-root",
          contentNodeIds: ["a"],
          content: `<img data-fleximark-node-id="a" src="fleximark-asset:${assetHash}">`,
          precondition: {
            nodeExists: true,
            currentParentId: "document-root",
          },
        },
      ]);
      assert.equal(preview.applyPatch(replacement), true);
      assert.equal(
        root.querySelector("img")?.getAttribute("src"),
        "blob:fleximark-top-level",
      );
    } finally {
      URL.createObjectURL = originalCreate;
      URL.revokeObjectURL = originalRevoke;
    }
  });

  test("rejects multiple roots and identity mismatches", () => {
    assert.equal(
      preview.applySnapshot({
        ...snapshot(),
        html: '<main data-fleximark-node-id="document-root"></main><p data-fleximark-node-id="a"></p>',
      }),
      false,
    );
  });

  test("rejects protected content before it reaches the live DOM", () => {
    assert.equal(
      preview.applySnapshot({
        ...snapshot(),
        html: '<main data-fleximark-node-id="document-root"><p data-fleximark-node-id="a" onclick="alert(1)">unsafe</p></main>',
      }),
      false,
    );
    assert.equal(root.innerHTML, "");
    assert.equal(requested, 1);
  });

  test("accepts inert Mermaid and ABC JSON payload scripts", () => {
    assert.equal(
      preview.applySnapshot({
        ...snapshot(),
        nodeIds: ["document-root", "mermaid", "abc"],
        navigation: [navigation("mermaid", 0), navigation("abc", 1)],
        html: '<main data-fleximark-node-id="document-root"><div data-fleximark-node-id="mermaid" data-fleximark-kind="mermaid"><script type="application/json">"graph TD; A--\\u003eB"</script></div><div data-fleximark-node-id="abc" data-fleximark-kind="abc"><script type="application/json">"X:1\\nK:C\\nC"</script></div></main>',
      }),
      true,
    );
    assert.equal(
      root.querySelectorAll('script[type="application/json"]').length,
      2,
    );
    assert.equal(requested, 0);
  });

  test("rejects executable and malformed data scripts", () => {
    for (const script of [
      "<script>alert(1)</script>",
      '<script type="application/json">not-json</script>',
      '<script type="application/json" src="https://evil.example/payload"></script>',
    ]) {
      assert.equal(
        preview.applySnapshot({
          ...snapshot(),
          html:
            '<main data-fleximark-node-id="document-root"><p data-fleximark-node-id="a">' +
            script +
            "</p></main>",
        }),
        false,
      );
    }
    assert.equal(root.innerHTML, "");
    assert.equal(requested, 3);
  });

  test("renders Mermaid payload into an SVG output", async () => {
    preview.applySnapshot(specialSnapshot("mermaid", "graph TD; A-->B"));
    const runtimes = inertRuntimes();
    runtimes.mermaid.render = async (_, source) => ({
      svg: `<svg data-source="${source}"></svg>`,
    });
    await new PreviewEnhancer(runtimes).render(root);
    assert.equal(
      root
        .querySelector("[data-fleximark-output] svg")
        ?.getAttribute("data-source"),
      "graph TD; A-->B",
    );
  });

  test("renders KaTeX-compatible math as MathML", async () => {
    preview.applySnapshot({
      ...specialSnapshot("math", ""),
      html: '<main data-fleximark-node-id="document-root"><div data-fleximark-node-id="special" data-fleximark-kind="math">x^2</div></main>',
    });
    const { previewRuntimes } =
      await import("../web/preview-client/runtimes.mjs");
    await new PreviewEnhancer(previewRuntimes).render(root);
    assert.ok(
      root.querySelector("[data-fleximark-output] math msup"),
      root.querySelector<HTMLElement>("[data-fleximark-kind=math]")?.dataset
        .fleximarkRenderError ?? root.innerHTML,
    );
  });

  test("renders ABC notation into an SVG output", async () => {
    preview.applySnapshot(specialSnapshot("abc", "X:1\nK:C\nC"));
    const { previewRuntimes } =
      await import("../web/preview-client/runtimes.mjs");
    await new PreviewEnhancer(previewRuntimes).render(root);
    assert.ok(root.querySelector("[data-fleximark-output] svg"));
  });

  test("starts ABC audio only after user action and cleans it up", async () => {
    preview.applySnapshot(specialSnapshot("abc", "X:1\nK:C\nC"));
    let started = 0;
    let stopped = 0;
    const runtimes = inertRuntimes();
    runtimes.abc.render = (target, source) => {
      target.innerHTML = `<svg data-source="${source.replaceAll("\n", " ")}"></svg>`;
      return [{}];
    };
    runtimes.abc.supportsAudio = () => true;
    runtimes.abc.createSynth = () => ({
      init: async () => undefined,
      prime: async () => undefined,
      start: () => started++,
      stop: () => stopped++,
    });
    const enhancer = new PreviewEnhancer(runtimes);
    await enhancer.render(root);
    assert.ok(root.querySelector("[data-fleximark-output] svg"));
    assert.equal(started, 0);
    (root.querySelector("[data-fleximark-audio]") as HTMLButtonElement).click();
    await new Promise((resolve) => setTimeout(resolve, 0));
    assert.equal(started, 1);
    await enhancer.render(root);
    assert.equal(stopped, 1);
    enhancer.dispose();
    assert.equal(stopped, 1);
  });

  test("loads only validated youtube-nocookie embeds after explicit consent", async () => {
    preview.applySnapshot({
      ...snapshot(),
      nodeIds: ["document-root", "youtube"],
      navigation: [navigation("youtube", 0)],
      html: '<main data-fleximark-node-id="document-root"><div data-fleximark-node-id="youtube" data-fleximark-kind="youtube" data-source="https://www.youtube.com/watch?v=dQw4w9WgXcQ"></div></main>',
    });
    const enhancer = new PreviewEnhancer(inertRuntimes());
    await enhancer.render(root);
    assert.equal(root.querySelector("iframe"), null);
    const consent = root.querySelector<HTMLButtonElement>(
      "[data-fleximark-youtube-consent]",
    );
    assert.ok(consent);
    consent.click();
    assert.equal(
      root.querySelector("iframe")?.getAttribute("src"),
      "https://www.youtube-nocookie.com/embed/dQw4w9WgXcQ",
    );
    enhancer.dispose();
  });

  test("activates typed tabs with accessible click and keyboard semantics", async () => {
    const tabs = snapshot();
    tabs.nodeIds = ["document-root", "tabs", "one", "two", "outside"];
    tabs.navigation = [
      navigation("tabs", 0),
      navigation("one", 1),
      navigation("two", 2),
      navigation("outside", 3),
    ];
    tabs.html =
      '<main data-fleximark-node-id="document-root"><section data-fleximark-node-id="tabs" data-fleximark-kind="tabs"><section data-fleximark-node-id="one" data-fleximark-kind="tab" data-tab-label="One">first</section><section data-fleximark-node-id="two" data-fleximark-kind="tab" data-tab-label="Two">second</section></section><p data-fleximark-node-id="outside">before</p></main>';
    assert.equal(preview.applySnapshot(tabs), true);
    const enhancer = new PreviewEnhancer(inertRuntimes());
    await enhancer.render(root);
    const buttons = [...root.querySelectorAll<HTMLButtonElement>("[role=tab]")];
    const panels = [...root.querySelectorAll<HTMLElement>("[role=tabpanel]")];
    assert.deepEqual(
      buttons.map((button) => button.textContent),
      ["One", "Two"],
    );
    assert.equal(buttons[0].getAttribute("aria-selected"), "true");
    assert.equal(panels[1].hidden, true);
    buttons[1].click();
    assert.equal(buttons[1].getAttribute("aria-selected"), "true");
    assert.equal(panels[0].hidden, true);
    const unrelated = patch([
      {
        type: "replace",
        nodeId: "outside",
        parentId: "document-root",
        contentNodeIds: ["outside"],
        content: '<p data-fleximark-node-id="outside">after</p>',
        precondition: {
          nodeExists: true,
          currentParentId: "document-root",
        },
      },
    ]);
    unrelated.navigation = tabs.navigation;
    assert.equal(preview.applyPatch(unrelated), true);
    await enhancer.render(root);
    assert.equal(
      root.querySelectorAll("[role=tab]")[1]?.getAttribute("aria-selected"),
      "true",
    );
    const currentButtons = [
      ...root.querySelectorAll<HTMLButtonElement>("[role=tab]"),
    ];
    const currentPanels = [
      ...root.querySelectorAll<HTMLElement>("[role=tabpanel]"),
    ];
    const left = new Event("keydown", { bubbles: true, cancelable: true });
    Object.defineProperty(left, "key", { value: "ArrowLeft" });
    currentButtons[1].dispatchEvent(left);
    assert.equal(currentButtons[0].getAttribute("aria-selected"), "true");
    assert.equal(currentPanels[0].hidden, false);
  });

  test("highlights a small local language set and safely leaves unknown code plain", async () => {
    preview.applySnapshot({
      ...snapshot(),
      html: '<main data-fleximark-node-id="document-root"><pre data-fleximark-node-id="a"><code class="language-ts">const value = "&lt;safe&gt;";</code></pre></main>',
    });
    const enhancer = new PreviewEnhancer(inertRuntimes());
    await enhancer.render(root);
    const code = root.querySelector("code");
    assert.equal(code?.dataset.fleximarkHighlighted, "ts");
    assert.ok(code?.querySelector(".fleximark-token-keyword"));
    assert.equal(code?.textContent, 'const value = "<safe>";');
    assert.ok(code);
    code.className = "language-unknown";
    code.replaceChildren("plain <code>");
    await enhancer.render(root);
    assert.equal(code.dataset.fleximarkHighlighted, undefined);
    assert.equal(code?.textContent, "plain <code>");
    enhancer.dispose();
  });

  test("preserves rendered line spans while highlighting numbered code", async () => {
    preview.applySnapshot({
      ...snapshot(),
      html: '<main data-fleximark-node-id="document-root"><figure data-fleximark-node-id="a" data-fleximark-kind="code"><figcaption class="fleximark-code-title">Demo</figcaption><pre><code class="language-rust"><span class="fleximark-code-line" data-line="1">fn main() {}</span>\n<span class="fleximark-code-line" data-line="2">let x = 2;</span></code></pre></figure></main>',
    });
    await new PreviewEnhancer(inertRuntimes()).render(root);
    const lines = root.querySelectorAll(".fleximark-code-line");
    assert.equal(lines.length, 2);
    assert.ok(lines[0].querySelector(".fleximark-token-keyword"));
    assert.equal(root.querySelector("figcaption")?.textContent, "Demo");
  });

  test("maps editor selection and viewport events to preview nodes", () => {
    preview.applySnapshot(snapshot());
    const events: object[] = [];
    const navigationClient = new PreviewNavigation(root, (event) =>
      events.push(event),
    );
    navigationClient.setKnownIds(["a"]);
    const node = root.querySelector<HTMLElement>("[data-fleximark-node-id=a]");
    assert.ok(node);
    let revealed = 0;
    node.scrollIntoView = () => revealed++;
    navigationClient.receive({ type: "selection", nodeIds: ["a"] });
    navigationClient.receive({ type: "viewport", nodeId: "a" });
    assert.equal(node.dataset.fleximarkSelected, "true");
    assert.equal(revealed, 1);
    node.dispatchEvent(new Event("click", { bubbles: true }));
    assert.deepEqual(events, [{ type: "selectNode", nodeId: "a" }]);
    navigationClient.dispose();
  });

  test("envelopes node events with the last successfully applied revision", () => {
    const events: object[] = [];
    const host = new PreviewHost(
      root,
      () => undefined,
      (event) => events.push(event),
    );
    host.apply([{ ...snapshot(), resultRenderRevision: 7 }]);
    root
      .querySelector<HTMLElement>("[data-fleximark-node-id=a]")
      ?.dispatchEvent(new Event("click", { bubbles: true }));
    assert.deepEqual(events, [
      {
        type: "selectNode",
        nodeId: "a",
        previewSessionId: "preview-1",
        renderRevision: 7,
      },
    ]);
    host.dispose();
  });

  test("accepts VS Code host messages and rejects cross-frame or malformed messages", () => {
    assert.equal(isPreviewHostMessage(undefined), false);
    assert.equal(isPreviewHostMessage({ type: "initializePreview" }), false);
    assert.equal(
      isPreviewHostMessage({
        type: "previewEvent",
        messageToken: "token",
        event: { type: "youtube" },
      }),
      false,
    );
    assert.equal(
      isPreviewHostMessage({
        type: "initializePreview",
        messageToken: "token",
        publication: snapshot(),
      }),
      true,
    );
    assert.equal(
      isPreviewHostMessage({
        type: "previewEvent",
        messageToken: "token",
        event: {
          type: "viewport",
          previewSessionId: "preview-1",
          renderRevision: 1,
          nodeId: "a",
        },
      }),
      true,
    );
    const valid = {
      type: "initializePreview",
      messageToken: "token",
      publication: snapshot(),
    };
    assert.equal(isPreviewHostMessageEvent(valid, "token"), true);
    assert.equal(isPreviewHostMessageEvent(valid, "other-token"), false);
    assert.equal(isPreviewHostMessageEvent({}, "token"), false);
  });

  test("debounces preview scroll and suppresses viewport echo", async () => {
    preview.applySnapshot(snapshot());
    const events: object[] = [];
    const navigationClient = new PreviewNavigation(root, (event) =>
      events.push(event),
    );
    navigationClient.setKnownIds(["a"]);
    const node = root.querySelector<HTMLElement>("[data-fleximark-node-id=a]");
    assert.ok(node);
    node.scrollIntoView = () => undefined;
    node.getBoundingClientRect = () => ({ bottom: 10 }) as DOMRect;
    navigationClient.receive({ type: "viewport", nodeId: "a" });
    window.dispatchEvent(new Event("scroll"));
    await new Promise((resolve) => setTimeout(resolve, 120));
    assert.deepEqual(events, []);
    window.dispatchEvent(new Event("scroll"));
    window.dispatchEvent(new Event("scroll"));
    await new Promise((resolve) => setTimeout(resolve, 120));
    assert.deepEqual(events, [{ type: "revealNode", nodeId: "a" }]);
    navigationClient.dispose();
  });
}

function specialSnapshot(
  kind: "mermaid" | "abc" | "math",
  source: string,
): RenderSnapshot {
  return {
    ...snapshot(),
    nodeIds: ["document-root", "special"],
    navigation: [navigation("special", 0)],
    html: `<main data-fleximark-node-id="document-root"><div data-fleximark-node-id="special" data-fleximark-kind="${kind}"><script type="application/json">${JSON.stringify(source).replaceAll("<", "\\u003c")}</script></div></main>`,
  };
}

function inertRuntimes(): PreviewRuntimes {
  return {
    mermaid: {
      initialize: () => undefined,
      render: async () => ({ svg: "<svg></svg>" }),
    },
    abc: {
      render: () => [],
      supportsAudio: () => false,
      createSynth: () => ({
        init: async () => undefined,
        prime: async () => undefined,
        start: () => undefined,
        stop: () => undefined,
      }),
    },
    math: { render: () => undefined },
  };
}

function snapshot(): RenderSnapshot {
  return {
    type: "full",
    previewSessionId: "preview-1",
    documentVersion: 1,
    resultRenderRevision: 1,
    rendererFingerprint: "sha256:renderer",
    nodeIds: ["document-root", "a"],
    navigation: [navigation("a", 0)],
    style: null,
    assets: [],
    html: '<main data-fleximark-node-id="document-root"><p data-fleximark-node-id="a">one</p></main>',
  };
}

function patch(operations: RenderPatch["operations"]): RenderPatch {
  return {
    type: "patch",
    previewSessionId: "preview-1",
    documentVersion: 2,
    baseRenderRevision: 1,
    resultRenderRevision: 2,
    baseRendererFingerprint: "sha256:renderer",
    resultRendererFingerprint: "sha256:renderer",
    navigation: [navigation("a", 0)],
    style: null,
    operations,
  };
}

function navigation(nodeId: string, line: number) {
  return {
    nodeId,
    sourceRange: {
      byteStart: 0,
      byteEnd: 3,
      start: { line, character: 0, encoding: "utf8" as const },
      end: { line, character: 3, encoding: "utf8" as const },
    },
    depth: 0,
  };
}

function asset(data: string) {
  return {
    reference: `fleximark-asset:${assetHash}`,
    mediaType: "image/png",
    contentHash: assetHash,
    byteLength: 5,
    data,
  };
}

const assetHash =
  "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824";
