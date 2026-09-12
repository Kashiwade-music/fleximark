import { parseHTML } from "linkedom";
import * as assert from "node:assert/strict";

import {
  PreviewEnhancer,
  type PreviewRuntimes,
} from "../web/preview-client/enhance.mjs";
import {
  stopAudio,
  trackAudio,
} from "../web/preview-client/enhancers/audio.mjs";
import {
  PreviewHost,
  isInvalidAuthenticatedPublicationMessage,
  isPreviewHostMessage,
  isPreviewHostMessageEvent,
} from "../web/preview-client/host.mjs";
import {
  PreviewDocument,
  type RenderPatch,
  type RenderSnapshot,
} from "../web/preview-client/index.mjs";
import { PreviewNavigation } from "../web/preview-client/navigation.mjs";
import { shouldForwardEditorNavigation } from "../web/preview-client/protocol.mjs";
import { previewRuntimes } from "../web/preview-client/runtimes.mjs";

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

  test("rolls back staged asset URLs and enforces the exact byte limits", () => {
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
      const initial = snapshot();
      initial.assets = [asset("aGVsbG8=")];
      initial.html = `<main data-fleximark-node-id="document-root"><img data-fleximark-node-id="a" src="fleximark-asset:${assetHash}"></main>`;
      assert.equal(preview.applySnapshot(initial), true);
      const before = root.innerHTML;

      const stagedHash = "1".repeat(64);
      const invalidReplacement = {
        ...snapshot(),
        resultRenderRevision: 2,
        assets: [sizedAsset(stagedHash, 5)],
        html: `<main data-fleximark-node-id="document-root"><img data-fleximark-node-id="a" src="fleximark-asset:${"2".repeat(64)}"></main>`,
      };
      assert.equal(preview.applySnapshot(invalidReplacement), false);
      assert.equal(root.innerHTML, before);
      assert.equal(preview.renderRevision, 1);
      assert.deepEqual(revoked, ["blob:fleximark-2"]);

      const oneMebibyte = 1024 * 1024;
      const exactLimit = {
        ...snapshot(),
        previewSessionId: "limit-preview",
        assets: [sizedAsset("3".repeat(64), oneMebibyte)],
      };
      assert.equal(preview.applySnapshot(exactLimit), true);
      assert.equal(preview.previewSessionId, "limit-preview");
      const exactTotalLimit = {
        ...exactLimit,
        previewSessionId: "total-limit-preview",
        assets: Array.from({ length: 8 }, (_, index) =>
          sizedAsset(String(index + 1).padStart(64, "a"), oneMebibyte),
        ),
      };
      assert.equal(preview.applySnapshot(exactTotalLimit), true);
      assert.equal(preview.previewSessionId, "total-limit-preview");
      assert.equal(
        preview.applySnapshot({
          ...exactTotalLimit,
          previewSessionId: "oversized-preview",
          assets: [sizedAsset("4".repeat(64), oneMebibyte + 1)],
        }),
        false,
      );
      assert.equal(preview.previewSessionId, "total-limit-preview");

      assert.equal(
        preview.applySnapshot({
          ...exactTotalLimit,
          previewSessionId: "total-overflow-preview",
          assets: Array.from({ length: 9 }, (_, index) =>
            sizedAsset(String(index + 1).padStart(64, "a"), oneMebibyte),
          ),
        }),
        false,
      );
      assert.equal(preview.previewSessionId, "total-limit-preview");
      assert.equal(requested, 3);
    } finally {
      URL.createObjectURL = originalCreate;
      URL.revokeObjectURL = originalRevoke;
    }
  });

  test("rejects every patch identity and fingerprint mismatch without mutation", () => {
    assert.equal(preview.applySnapshot(snapshot()), true);
    const before = root.innerHTML;
    const cases: RenderPatch[] = [
      { ...patch([]), previewSessionId: "other-preview" },
      { ...patch([]), baseRenderRevision: 0 },
      { ...patch([]), baseRendererFingerprint: "other-renderer" },
      { ...patch([]), resultRendererFingerprint: "other-renderer" },
    ];
    for (const candidate of cases) {
      assert.equal(preview.applyPatch(candidate), false);
      assert.equal(root.innerHTML, before);
      assert.equal(preview.previewSessionId, "preview-1");
      assert.equal(preview.renderRevision, 1);
    }
    assert.equal(requested, cases.length);
  });

  test("ignores a stale same-session patch without requesting a snapshot", () => {
    assert.equal(preview.applySnapshot(snapshot()), true);
    const before = root.innerHTML;
    assert.equal(
      preview.applyPatch({ ...patch([]), resultRenderRevision: 1 }),
      false,
    );
    assert.equal(root.innerHTML, before);
    assert.equal(preview.renderRevision, 1);
    assert.equal(requested, 0);
  });

  test("clears pending patch highlight timers during disposal", () => {
    assert.equal(preview.applySnapshot(snapshot()), true);
    assert.equal(
      preview.applyPatch(
        patch([
          {
            type: "replace",
            nodeId: "a",
            parentId: "document-root",
            contentNodeIds: ["a"],
            content: '<p data-fleximark-node-id="a">highlighted</p>',
            precondition: {
              nodeExists: true,
              currentParentId: "document-root",
            },
          },
        ]),
      ),
      true,
    );
    const highlighted = root.querySelector<HTMLElement>(
      "[data-fleximark-node-id=a]",
    );
    assert.equal(highlighted?.classList.contains("fade-highlight"), true);
    preview.dispose();
    assert.equal(highlighted?.classList.contains("fade-highlight"), false);
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

  test("rejects the complete executable-content surface and permits inert data images", () => {
    const unsafe = [
      "<base href='https://evil.example/'>",
      "<embed src='https://evil.example/payload'>",
      "<iframe src='https://evil.example/frame'></iframe>",
      "<link rel='stylesheet' href='https://evil.example/style.css'>",
      "<meta http-equiv='refresh' content='0;url=https://evil.example'>",
      "<object data='https://evil.example/payload'></object>",
      "<img onerror='alert(1)'>",
      "<div style='color:red'></div>",
      "<iframe srcdoc='<script>alert(1)</script>'></iframe>",
      "<a href=' JAVASCRIPT:alert(1)'>x</a>",
      "<a href='vbscript:msgbox(1)'>x</a>",
      "<a href='file:///secret'>x</a>",
      "<a href='data:text/html,evil'>x</a>",
      "<img src='data:text/html,evil'>",
    ];
    for (const content of unsafe)
      assert.equal(
        preview.applySnapshot({
          ...snapshot(),
          html: `<main data-fleximark-node-id="document-root"><p data-fleximark-node-id="a">${content}</p></main>`,
        }),
        false,
        content,
      );
    assert.equal(root.innerHTML, "");
    assert.equal(requested, unsafe.length);

    assert.equal(
      preview.applySnapshot({
        ...snapshot(),
        html: '<main data-fleximark-node-id="document-root"><img data-fleximark-node-id="a" src="data:image/png;base64,aGVsbG8="></main>',
      }),
      true,
    );
    assert.equal(requested, unsafe.length);
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

  test("isolates enhancer failures and retries only the failed block", async () => {
    const publication = snapshot();
    publication.nodeIds = ["document-root", "mermaid", "math"];
    publication.navigation = [navigation("mermaid", 0), navigation("math", 1)];
    publication.html =
      '<main data-fleximark-node-id="document-root"><div data-fleximark-node-id="mermaid" data-fleximark-kind="mermaid"><script type="application/json">"graph TD; A--&gt;B"</script></div><div data-fleximark-node-id="math" data-fleximark-kind="math">x</div></main>';
    assert.equal(preview.applySnapshot(publication), true);
    const runtimes = inertRuntimes();
    let mermaidAttempts = 0;
    let mathAttempts = 0;
    runtimes.mermaid.render = async () => {
      mermaidAttempts += 1;
      if (mermaidAttempts === 1) throw new Error("mermaid failed");
      return { svg: "<svg data-recovered='true'></svg>" };
    };
    runtimes.math.render = (_source, target) => {
      mathAttempts += 1;
      target.dataset.mathRendered = "true";
    };
    const enhancer = new PreviewEnhancer(runtimes);

    await enhancer.render(root);
    const mermaid = root.querySelector<HTMLElement>(
      '[data-fleximark-node-id="mermaid"]',
    );
    const math = root.querySelector<HTMLElement>(
      '[data-fleximark-node-id="math"]',
    );
    assert.equal(mermaid?.dataset.fleximarkRenderError, "mermaid failed");
    assert.equal(math?.dataset.fleximarkRenderError, undefined);
    assert.equal(
      math?.querySelector<HTMLElement>("[data-fleximark-output]")?.dataset
        .mathRendered,
      "true",
    );

    await enhancer.render(root);
    assert.equal(mermaidAttempts, 2);
    assert.equal(mathAttempts, 1);
    assert.equal(mermaid?.dataset.fleximarkRenderError, undefined);
    assert.ok(mermaid?.querySelector("svg[data-recovered=true]"));
    enhancer.dispose();
  });

  test("drops a stale asynchronous Mermaid result after disposal", async () => {
    preview.applySnapshot(specialSnapshot("mermaid", "graph TD; A-->B"));
    const code = document.createElement("code");
    code.className = "language-ts";
    code.textContent = "const stale = true";
    const pre = document.createElement("pre");
    pre.append(code);
    root.append(pre);
    let resolveRender!: (value: { svg: string }) => void;
    const deferred = new Promise<{ svg: string }>((resolve) => {
      resolveRender = resolve;
    });
    const runtimes = inertRuntimes();
    runtimes.mermaid.render = () => deferred;
    const enhancer = new PreviewEnhancer(runtimes);
    const rendering = enhancer.render(root);
    enhancer.dispose();
    resolveRender({ svg: "<svg data-stale='true'></svg>" });
    await rendering;

    assert.equal(root.querySelector("svg[data-stale=true]"), null);
    assert.equal(
      root.querySelector<HTMLElement>("[data-fleximark-kind=mermaid]")?.dataset
        .fleximarkRenderError,
      undefined,
    );
    assert.equal(code.dataset.fleximarkHighlighted, undefined);
  });

  test("does not render again after enhancer disposal", async () => {
    preview.applySnapshot(specialSnapshot("mermaid", "A"));
    let renderCount = 0;
    const runtimes = inertRuntimes();
    runtimes.mermaid.render = async () => {
      renderCount += 1;
      return { svg: "<svg></svg>" };
    };
    const enhancer = new PreviewEnhancer(runtimes);
    enhancer.dispose();
    await enhancer.render(root);
    assert.equal(renderCount, 0);
    assert.equal(root.querySelector("[data-fleximark-output]"), null);
  });

  test("keeps the newest Mermaid fingerprint across reverse completion order", async () => {
    preview.applySnapshot(specialSnapshot("mermaid", "A"));
    let resolveFirst!: (value: { svg: string }) => void;
    let resolveSecond!: (value: { svg: string }) => void;
    const first = new Promise<{ svg: string }>((resolve) => {
      resolveFirst = resolve;
    });
    const second = new Promise<{ svg: string }>((resolve) => {
      resolveSecond = resolve;
    });
    const sources: string[] = [];
    const runtimes = inertRuntimes();
    runtimes.mermaid.render = (_id, source) => {
      sources.push(source);
      if (sources.length === 1) return first;
      if (sources.length === 2) return second;
      return Promise.resolve({ svg: `<svg data-source="${source}"></svg>` });
    };
    const enhancer = new PreviewEnhancer(runtimes);
    const block = root.querySelector<HTMLElement>(
      "[data-fleximark-kind=mermaid]",
    );
    const payload = block?.querySelector("script");
    assert.ok(block);
    assert.ok(payload);

    const renderingA = enhancer.render(root);
    payload.textContent = JSON.stringify("B");
    const renderingB = enhancer.render(root);
    resolveSecond({ svg: '<svg data-source="B"></svg>' });
    await renderingB;
    resolveFirst({ svg: '<svg data-source="stale-A"></svg>' });
    await renderingA;
    assert.equal(block.dataset.fleximarkRenderError, undefined);
    assert.equal(block.querySelector("svg")?.getAttribute("data-source"), "B");

    payload.textContent = JSON.stringify("A");
    await enhancer.render(root);
    assert.deepEqual(sources, ["A", "B", "A"]);
    assert.equal(block.querySelector("svg")?.getAttribute("data-source"), "A");
    enhancer.dispose();
  });

  test("retries transient audio cleanup without delaying other handles", () => {
    let transientStops = 0;
    let stableStops = 0;
    const transient = trackAudio({
      init: async () => undefined,
      prime: async () => undefined,
      start: () => undefined,
      stop: () => {
        transientStops += 1;
        if (transientStops === 1) throw new Error("temporary stop failure");
      },
    });
    const stable = trackAudio({
      init: async () => undefined,
      prime: async () => undefined,
      start: () => undefined,
      stop: () => {
        stableStops += 1;
      },
    });
    const handles = new Set([transient, stable]);

    stopAudio(handles);
    assert.equal(transientStops, 1);
    assert.equal(stableStops, 1);
    assert.deepEqual([...handles], [transient]);

    stopAudio(handles);
    stopAudio(handles);
    assert.equal(transientStops, 2);
    assert.equal(stableStops, 1);
    assert.equal(handles.size, 0);
  });

  test("observes an asynchronous audio stop and retries only after rejection", async () => {
    const originalRender = previewRuntimes.abc.render;
    const originalSupportsAudio = previewRuntimes.abc.supportsAudio;
    const originalCreateSynth = previewRuntimes.abc.createSynth;
    let rejectFirstStop!: (error: Error) => void;
    let resolveSecondStop!: () => void;
    const firstStop = new Promise<void>((_resolve, reject) => {
      rejectFirstStop = reject;
    });
    const secondStop = new Promise<void>((resolve) => {
      resolveSecondStop = resolve;
    });
    let stopAttempts = 0;
    const host = new PreviewHost(
      root,
      () => undefined,
      () => undefined,
    );
    try {
      previewRuntimes.abc.render = () => [{}];
      previewRuntimes.abc.supportsAudio = () => true;
      previewRuntimes.abc.createSynth = () => ({
        init: async () => undefined,
        prime: async () => undefined,
        start: () => undefined,
        stop: () => {
          stopAttempts += 1;
          return stopAttempts === 1 ? firstStop : secondStop;
        },
      });
      host.apply([specialSnapshot("abc", "X:1\nK:C\nC")]);
      root.querySelector<HTMLButtonElement>("[data-fleximark-audio]")?.click();
      await flushMicrotasks();

      host.dispose();
      host.dispose();
      assert.equal(stopAttempts, 1);
      rejectFirstStop(new Error("asynchronous stop failure"));
      await flushMicrotasks();

      host.dispose();
      host.dispose();
      assert.equal(stopAttempts, 2);
      resolveSecondStop();
      await flushMicrotasks();
      host.dispose();
      assert.equal(stopAttempts, 2);
    } finally {
      host.dispose();
      previewRuntimes.abc.render = originalRender;
      previewRuntimes.abc.supportsAudio = originalSupportsAudio;
      previewRuntimes.abc.createSynth = originalCreateSynth;
    }
  });

  test("closes the real ABC audio runtime after every partial start failure", async () => {
    const originalAudioContext = Object.getOwnPropertyDescriptor(
      globalThis,
      "AudioContext",
    );
    try {
      for (const failedStage of ["setup", "meter", "oscillator"] as const) {
        let closeCount = 0;
        let oscillatorStopCount = 0;
        let oscillatorDisconnectCount = 0;
        class FakeAudioContext {
          readonly currentTime = 0;
          readonly destination = {};

          createOscillator() {
            return {
              frequency: { value: 0 },
              connect: () => ({ connect: () => undefined }),
              start: () => undefined,
              stop: () => {
                oscillatorStopCount += 1;
              },
              disconnect: () => {
                oscillatorDisconnectCount += 1;
              },
            };
          }

          createGain(): never {
            throw new Error("oscillator failed");
          }

          close(): Promise<void> {
            closeCount += 1;
            return Promise.resolve();
          }
        }
        Object.defineProperty(globalThis, "AudioContext", {
          configurable: true,
          value: FakeAudioContext,
        });
        const visual = {
          setUpAudio: () => {
            if (failedStage === "setup") throw new Error("setup failed");
            return {
              tempo: 120,
              tracks: [
                [
                  {
                    cmd: "note",
                    start: 0,
                    duration: 1,
                    pitch: 69,
                    volume: 100,
                  },
                ],
              ],
            };
          },
          getMeterFraction: () => {
            if (failedStage === "meter") throw new Error("meter failed");
            return { num: 4, den: 4 };
          },
          millisecondsPerMeasure: () => 1000,
        };
        const synth = previewRuntimes.abc.createSynth();
        await synth.init({ visualObj: visual });
        assert.throws(() => synth.start(), new Error(`${failedStage} failed`));
        await flushMicrotasks();
        assert.equal(closeCount, 1, failedStage);
        assert.equal(
          oscillatorStopCount,
          failedStage === "oscillator" ? 1 : 0,
          failedStage,
        );
        assert.equal(
          oscillatorDisconnectCount,
          failedStage === "oscillator" ? 1 : 0,
          failedStage,
        );
        await assert.doesNotReject(Promise.resolve(synth.stop()));
        assert.equal(closeCount, 1, failedStage);
      }
    } finally {
      if (originalAudioContext)
        Object.defineProperty(globalThis, "AudioContext", originalAudioContext);
      else Reflect.deleteProperty(globalThis, "AudioContext");
    }
  });

  test("coalesces pending AudioContext close and retries after rejection", async () => {
    const originalAudioContext = Object.getOwnPropertyDescriptor(
      globalThis,
      "AudioContext",
    );
    let rejectFirstClose!: (error: Error) => void;
    let resolveSecondClose!: () => void;
    const firstClose = new Promise<void>((_resolve, reject) => {
      rejectFirstClose = reject;
    });
    const secondClose = new Promise<void>((resolve) => {
      resolveSecondClose = resolve;
    });
    let closeAttempts = 0;
    class FakeAudioContext {
      close(): Promise<void> {
        closeAttempts += 1;
        return closeAttempts === 1 ? firstClose : secondClose;
      }
    }
    try {
      Object.defineProperty(globalThis, "AudioContext", {
        configurable: true,
        value: FakeAudioContext,
      });
      const synth = previewRuntimes.abc.createSynth();
      await synth.init({
        visualObj: {
          setUpAudio: () => {
            throw new Error("start failed");
          },
        },
      });
      assert.throws(() => synth.start(), new Error("start failed"));
      const coalescedClose = Promise.resolve(synth.stop());
      assert.equal(closeAttempts, 1);

      rejectFirstClose(new Error("close rejected"));
      await assert.rejects(coalescedClose, new Error("close rejected"));
      const retry = Promise.resolve(synth.stop());
      const coalescedRetry = Promise.resolve(synth.stop());
      assert.equal(closeAttempts, 2);

      resolveSecondClose();
      await Promise.all([retry, coalescedRetry]);
      await assert.doesNotReject(Promise.resolve(synth.stop()));
      assert.equal(closeAttempts, 2);
    } finally {
      if (originalAudioContext)
        Object.defineProperty(globalThis, "AudioContext", originalAudioContext);
      else Reflect.deleteProperty(globalThis, "AudioContext");
    }
  });

  test("stops pending ABC audio on disposal and never starts it", async () => {
    preview.applySnapshot(specialSnapshot("abc", "X:1\nK:C\nC"));
    let resolveInit!: () => void;
    const pendingInit = new Promise<void>((resolve) => {
      resolveInit = resolve;
    });
    let started = 0;
    let stopped = 0;
    const runtimes = inertRuntimes();
    runtimes.abc.render = () => [{}];
    runtimes.abc.supportsAudio = () => true;
    runtimes.abc.createSynth = () => ({
      init: () => pendingInit,
      prime: async () => undefined,
      start: () => started++,
      stop: () => stopped++,
    });
    const enhancer = new PreviewEnhancer(runtimes);
    await enhancer.render(root);
    const button = root.querySelector<HTMLButtonElement>(
      "[data-fleximark-audio]",
    );
    assert.ok(button);
    button.click();
    assert.equal(button.disabled, true);
    enhancer.dispose();
    resolveInit();
    await pendingInit;
    await Promise.resolve();

    assert.equal(started, 0);
    assert.equal(stopped, 1);
    assert.equal(button.disabled, true);
  });

  test("isolates and retries ABC init, prime, and start failures", async () => {
    for (const failedStage of ["init", "prime", "start"] as const) {
      preview.applySnapshot({
        ...specialSnapshot("abc", "X:1\nK:C\nC"),
        previewSessionId: `preview-${failedStage}`,
      });
      let synthAttempt = 0;
      let starts = 0;
      let stops = 0;
      const runtimes = inertRuntimes();
      runtimes.abc.render = () => [{}];
      runtimes.abc.supportsAudio = () => true;
      runtimes.abc.createSynth = () => {
        synthAttempt += 1;
        const fail = synthAttempt === 1;
        return {
          init: async () => {
            if (fail && failedStage === "init") throw new Error("init failed");
          },
          prime: async () => {
            if (fail && failedStage === "prime")
              throw new Error("prime failed");
          },
          start: () => {
            if (fail && failedStage === "start")
              throw new Error("start failed");
            starts += 1;
          },
          stop: () => {
            stops += 1;
          },
        };
      };
      const enhancer = new PreviewEnhancer(runtimes);
      await enhancer.render(root);
      root.querySelector<HTMLButtonElement>("[data-fleximark-audio]")?.click();
      await flushMicrotasks();
      const block = root.querySelector<HTMLElement>(
        "[data-fleximark-kind=abc]",
      );
      assert.equal(
        block?.dataset.fleximarkRenderError,
        `${failedStage} failed`,
      );
      assert.equal(stops, 1);

      await enhancer.render(root);
      root.querySelector<HTMLButtonElement>("[data-fleximark-audio]")?.click();
      await flushMicrotasks();
      assert.equal(block?.dataset.fleximarkRenderError, undefined);
      assert.equal(starts, 1);
      enhancer.dispose();
      assert.equal(stops, 2);
    }
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
    let observeStart!: () => void;
    const startSignal = new Promise<void>((resolve) => {
      observeStart = resolve;
    });
    const runtimes = inertRuntimes();
    runtimes.abc.render = (target, source) => {
      target.innerHTML = `<svg data-source="${source.replaceAll("\n", " ")}"></svg>`;
      return [{}];
    };
    runtimes.abc.supportsAudio = () => true;
    runtimes.abc.createSynth = () => ({
      init: async () => undefined,
      prime: async () => undefined,
      start: () => {
        started += 1;
        observeStart();
      },
      stop: () => stopped++,
    });
    const enhancer = new PreviewEnhancer(runtimes);
    await enhancer.render(root);
    assert.ok(root.querySelector("[data-fleximark-output] svg"));
    assert.equal(started, 0);
    (root.querySelector("[data-fleximark-audio]") as HTMLButtonElement).click();
    await waitForSignal(startSignal, "ABC audio start");
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

  test("ignores stale YouTube consent controls after rerender and disposal", async () => {
    preview.applySnapshot({
      ...snapshot(),
      nodeIds: ["document-root", "youtube"],
      navigation: [navigation("youtube", 0)],
      html: '<main data-fleximark-node-id="document-root"><div data-fleximark-node-id="youtube" data-fleximark-kind="youtube" data-source="https://youtu.be/dQw4w9WgXcQ"></div></main>',
    });
    const enhancer = new PreviewEnhancer(inertRuntimes());
    await enhancer.render(root);
    const stale = root.querySelector<HTMLButtonElement>(
      "[data-fleximark-youtube-consent]",
    );
    assert.ok(stale);
    await enhancer.render(root);
    const current = root.querySelector<HTMLButtonElement>(
      "[data-fleximark-youtube-consent]",
    );
    assert.ok(current);
    assert.notEqual(current, stale);
    stale.click();
    assert.equal(root.querySelector("iframe"), null);
    enhancer.dispose();
    current.click();
    assert.equal(root.querySelector("iframe"), null);
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

  test("requests snapshots only for authenticated malformed publications", () => {
    assert.equal(
      isInvalidAuthenticatedPublicationMessage(
        {
          type: "previewEvent",
          messageToken: "token",
          event: { type: "full", previewSessionId: "preview" },
        },
        "token",
      ),
      true,
    );
    for (const publication of [undefined, null, 7, "invalid"]) {
      assert.equal(
        isInvalidAuthenticatedPublicationMessage(
          {
            type: "initializePreview",
            messageToken: "token",
            ...(publication === undefined ? {} : { publication }),
          },
          "token",
        ),
        true,
      );
    }
    for (const value of [
      {
        type: "previewEvent",
        messageToken: "token",
        event: { type: "selection", nodeIds: "invalid" },
      },
      {
        type: "previewEvent",
        messageToken: "token",
        event: { type: "unknown" },
      },
      {
        type: "previewEvent",
        messageToken: "other",
        event: { type: "patch" },
      },
    ])
      assert.equal(
        isInvalidAuthenticatedPublicationMessage(value, "token"),
        false,
      );
  });

  test("forwards editor navigation only for the current preview revision", () => {
    const current = {
      type: "selectNode",
      previewSessionId: "preview",
      renderRevision: 4,
      nodeId: "a",
    };
    assert.equal(shouldForwardEditorNavigation(current, "preview", 4), true);
    assert.equal(shouldForwardEditorNavigation(current, "other", 4), false);
    assert.equal(shouldForwardEditorNavigation(current, "preview", 5), false);
  });

  test("accepts Rust selection wire with null or object activePosition", () => {
    for (const event of [
      {
        type: "selection",
        previewSessionId: "preview",
        renderRevision: 1,
        nodeIds: ["document-root"],
        activePosition: null,
      },
      {
        type: "selection",
        previewSessionId: "preview",
        renderRevision: 2,
        nodeIds: ["paragraph"],
        activePosition: { line: 3, character: 5 },
      },
    ]) {
      const message = {
        type: "previewEvent",
        messageToken: "token",
        event,
      };
      assert.equal(
        isPreviewHostMessage(JSON.parse(JSON.stringify(message))),
        true,
      );
    }
  });

  test("requests a full snapshot and preserves DOM for nested malformed publication data", () => {
    assert.equal(preview.applySnapshot(snapshot()), true);
    const beforeDom = root.innerHTML;
    const beforeNavigation = [...preview.navigation];
    const malformedNavigation = {
      ...snapshot(),
      resultRenderRevision: 2,
      navigation: [
        {
          ...navigation("a", 0),
          sourceRange: {
            ...navigation("a", 0).sourceRange,
            start: { line: 0, character: 0, encoding: "bytes" },
          },
        },
      ],
    } as unknown as RenderSnapshot;

    assert.equal(preview.applySnapshot(malformedNavigation), false);
    assert.equal(root.innerHTML, beforeDom);
    assert.deepEqual(preview.navigation, beforeNavigation);
    assert.equal(preview.renderRevision, 1);
    assert.equal(requested, 1);
  });

  test("stops a host batch after an invalid publication and requests one full snapshot", () => {
    const host = new PreviewHost(
      root,
      () => requested++,
      () => undefined,
    );
    const malformed = {
      ...patch([]),
      operations: [
        {
          type: "remove",
          nodeId: "missing",
          parentId: "document-root",
          precondition: {
            nodeExists: true,
            currentParentId: "document-root",
          },
        },
      ],
    } as RenderPatch;

    host.apply([
      snapshot(),
      malformed,
      { ...snapshot(), resultRenderRevision: 3, html: "not applied" },
    ]);

    assert.equal(host.renderRevision, 1);
    assert.match(root.innerHTML, />one</);
    assert.equal(requested, 1);
    host.dispose();
  });

  test("does not start enhancement after an invalid batch synchronously disposes the host", async () => {
    const originalRender = previewRuntimes.mermaid.render;
    let mermaidRenders = 0;
    previewRuntimes.mermaid.render = async () => {
      mermaidRenders += 1;
      return { svg: "<svg></svg>" };
    };
    const host = new PreviewHost(
      root,
      () => host.dispose(),
      () => undefined,
    );
    const malformed = {
      ...patch([]),
      operations: [
        {
          type: "remove",
          nodeId: "missing",
          parentId: "document-root",
          precondition: {
            nodeExists: true,
            currentParentId: "document-root",
          },
        },
      ],
    } as RenderPatch;
    try {
      host.apply([specialSnapshot("mermaid", "graph TD; A-->B"), malformed]);
      await Promise.resolve();
      assert.equal(host.renderRevision, 1);
      assert.equal(mermaidRenders, 0);
    } finally {
      host.dispose();
      previewRuntimes.mermaid.render = originalRender;
    }
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
    const originalSetTimeout = globalThis.setTimeout;
    const originalClearTimeout = globalThis.clearTimeout;
    const timers = new Map<number, { callback: () => void; delay: number }>();
    const scheduledDelays: number[] = [];
    let timerSequence = 0;
    globalThis.setTimeout = ((callback: TimerHandler, delay?: number) => {
      assert.equal(typeof callback, "function");
      timerSequence += 1;
      const recordedDelay = delay ?? 0;
      scheduledDelays.push(recordedDelay);
      timers.set(timerSequence, {
        callback: callback as () => void,
        delay: recordedDelay,
      });
      return timerSequence as unknown as ReturnType<typeof setTimeout>;
    }) as unknown as typeof setTimeout;
    globalThis.clearTimeout = ((timer: ReturnType<typeof setTimeout>) => {
      timers.delete(timer as unknown as number);
    }) as unknown as typeof clearTimeout;
    try {
      navigationClient.receive({ type: "viewport", nodeId: "a" });
      window.dispatchEvent(new Event("scroll"));
      assert.deepEqual(events, []);
      assert.equal(timers.size, 0);

      window.dispatchEvent(new Event("scroll"));
      window.dispatchEvent(new Event("scroll"));
      assert.equal(timers.size, 1);
      assert.deepEqual(scheduledDelays, [100, 100]);
      const pending = [...timers.entries()][0];
      assert.ok(pending);
      assert.equal(pending[1].delay, 100);
      timers.delete(pending[0]);
      pending[1].callback();
      assert.deepEqual(events, [{ type: "revealNode", nodeId: "a" }]);
    } finally {
      navigationClient.dispose();
      globalThis.setTimeout = originalSetTimeout;
      globalThis.clearTimeout = originalClearTimeout;
    }
  });
}

async function flushMicrotasks(): Promise<void> {
  await Promise.resolve();
  await Promise.resolve();
  await Promise.resolve();
}

async function waitForSignal(
  signal: Promise<void>,
  label: string,
): Promise<void> {
  let timeout: ReturnType<typeof setTimeout> | undefined;
  try {
    await Promise.race([
      signal,
      new Promise<never>((_, reject) => {
        timeout = setTimeout(
          () => reject(new Error(`Timed out waiting for ${label}`)),
          5_000,
        );
      }),
    ]);
  } finally {
    if (timeout !== undefined) clearTimeout(timeout);
  }
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

function sizedAsset(contentHash: string, byteLength: number) {
  return {
    reference: `fleximark-asset:${contentHash}`,
    mediaType: "image/png",
    contentHash,
    byteLength,
    data: Buffer.alloc(byteLength).toString("base64"),
  };
}

const assetHash =
  "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824";
