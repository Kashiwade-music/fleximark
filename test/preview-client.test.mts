import { parseHTML } from "linkedom";
import * as assert from "node:assert/strict";

import {
  PreviewEnhancer,
  type PreviewRuntimes,
} from "../web/preview-client/enhance.mjs";
import {
  PreviewHost,
  isInvalidAuthenticatedFrameMessage,
  isPreviewHostMessage,
  isPreviewHostMessageEvent,
} from "../web/preview-client/host.mjs";
import {
  PreviewDocument,
  type RenderAsset,
  type RenderFrame,
} from "../web/preview-client/index.mjs";
import { PreviewNavigation } from "../web/preview-client/navigation.mjs";
import { shouldForwardEditorNavigation } from "../web/preview-client/protocol.mjs";
import { deferred } from "./adapter/async-helpers.mjs";

export const suiteName = "Preview client";

export function suite(): void {
  let root: HTMLElement;
  let requested: number;
  let preview: PreviewDocument;

  setup(() => {
    const { window } = parseHTML(
      "<!doctype html><html><body><main id=preview></main></body></html>",
    );
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
    for (const key of [
      "document",
      "HTMLElement",
      "HTMLScriptElement",
      "Node",
      "Element",
      "Event",
      "window",
    ])
      Reflect.deleteProperty(globalThis, key);
  });

  test("applies complete frames and reuses unchanged block DOM", () => {
    assert.equal(
      preview.apply(frame(1, [block("a", "one"), block("b", "two")])),
      true,
    );
    const originalA = root.children[0];
    const originalB = root.children[1];

    assert.equal(
      preview.apply(frame(2, [block("b", "two"), block("a", "changed")])),
      true,
    );
    assert.equal(root.children[0], originalB);
    assert.notEqual(root.children[1], originalA);
    assert.equal(root.textContent, "twochanged");
    assert.equal(preview.renderRevision, 2);
  });

  test("does not mutate the child list for an unchanged complete frame", () => {
    const unchanged = {
      ...frame(1, [block("a", "one"), block("b", "two")]),
      annotations: { source: "stable" },
    };
    assert.equal(preview.apply(unchanged), true);
    const originalChildren = [...root.children];
    const observer = new window.MutationObserver(() => undefined);
    observer.observe(root, { childList: true });

    assert.equal(
      preview.apply({ ...unchanged, documentVersion: 2, renderRevision: 2 }),
      true,
    );
    assert.deepEqual(observer.takeRecords(), []);
    assert.deepEqual([...root.children], originalChildren);
    observer.disconnect();
  });

  test("reconciles one changed block without detaching 589 unchanged blocks", () => {
    const blocks = Array.from({ length: 590 }, (_, index) =>
      block(`block-${index}`, `value-${index}`),
    );
    assert.equal(preview.apply(frame(1, blocks)), true);
    const originals = [...root.children];
    const observer = new window.MutationObserver(() => undefined);
    observer.observe(root, { childList: true });

    const changed = [...blocks];
    changed[295] = block("block-295", "changed");
    assert.equal(preview.apply(frame(2, changed)), true);

    const records = observer.takeRecords();
    assert.equal(
      records.reduce((count, record) => count + record.removedNodes.length, 0),
      1,
    );
    assert.equal(
      records.reduce((count, record) => count + record.addedNodes.length, 0),
      1,
    );
    for (let index = 0; index < originals.length; index++) {
      if (index === 295)
        assert.notEqual(root.children[index], originals[index]);
      else assert.equal(root.children[index], originals[index]);
    }
    observer.disconnect();
  });

  test("reconciles insertion, deletion, and reorder while retaining identities", () => {
    assert.equal(
      preview.apply(
        frame(1, [
          block("a", "one"),
          block("b", "two"),
          block("c", "three"),
          block("d", "four"),
        ]),
      ),
      true,
    );
    const identities = new Map(
      [...root.children].map((element) => [
        (element as HTMLElement).dataset.fleximarkNodeId,
        element,
      ]),
    );

    assert.equal(
      preview.apply(
        frame(2, [
          block("a", "one"),
          block("x", "inserted"),
          block("b", "two"),
          block("c", "three"),
          block("d", "four"),
        ]),
      ),
      true,
    );
    assert.deepEqual(
      [...root.children].map(
        (element) => (element as HTMLElement).dataset.fleximarkNodeId,
      ),
      ["a", "x", "b", "c", "d"],
    );
    assert.equal(root.children[0], identities.get("a"));
    assert.equal(root.children[2], identities.get("b"));

    assert.equal(
      preview.apply(
        frame(3, [
          block("a", "one"),
          block("x", "inserted"),
          block("c", "three"),
          block("d", "four"),
        ]),
      ),
      true,
    );
    assert.equal(root.children[2], identities.get("c"));
    assert.equal(root.children[3], identities.get("d"));

    const x = root.children[1];
    assert.equal(
      preview.apply(
        frame(4, [
          block("a", "one"),
          block("d", "four"),
          block("c", "three"),
          block("x", "inserted"),
        ]),
      ),
      true,
    );
    assert.equal(root.children[0], identities.get("a"));
    assert.equal(root.children[1], identities.get("d"));
    assert.equal(root.children[2], identities.get("c"));
    assert.equal(root.children[3], x);
  });

  test("starts a new session with fresh DOM and rebuilds on a renderer change", () => {
    assert.equal(preview.apply(frame(7, [block("a", "one")])), true);
    const original = root.firstElementChild;
    assert.equal(
      preview.apply({
        ...frame(1, [block("a", "one")]),
        previewSessionId: "new-preview",
      }),
      true,
    );
    assert.notEqual(root.firstElementChild, original);
    assert.equal(preview.renderRevision, 1);

    const nextSession = root.firstElementChild;

    assert.equal(
      preview.apply({
        ...frame(2, [block("a", "one")]),
        previewSessionId: "new-preview",
        rendererFingerprint: "b".repeat(64),
      }),
      true,
    );
    assert.notEqual(root.firstElementChild, nextSession);
  });

  test("invalidates DOM reuse when renderer fingerprint changes", () => {
    assert.equal(preview.apply(frame(1, [block("a", "one")])), true);
    const original = root.firstElementChild;
    assert.equal(
      preview.apply({
        ...frame(2, [block("a", "one")]),
        rendererFingerprint: "b".repeat(64),
      }),
      true,
    );
    assert.notEqual(root.firstElementChild, original);
  });

  test("commits annotations as inert JSON and updates or removes them atomically", () => {
    const malicious = '</script><img data-injected src="x">';
    assert.equal(
      preview.apply({
        ...frame(1, [block("a", "safe")]),
        annotations: { plugin: malicious },
      }),
      true,
    );
    const original = root.querySelector<HTMLScriptElement>(
      "script[data-fleximark-render-annotations]",
    );
    assert.ok(original);
    assert.equal(original.type, "application/json");
    assert.equal(original.textContent, JSON.stringify({ plugin: malicious }));
    assert.equal(root.querySelector("img[data-injected]"), null);

    assert.equal(
      preview.apply({
        ...frame(2, [
          {
            id: "bad",
            nodeIds: ["bad"],
            html: '<script data-fleximark-node-id="bad">bad</script>',
          },
        ]),
        annotations: { plugin: "must-not-commit" },
      }),
      false,
    );
    assert.equal(
      root.querySelector("script[data-fleximark-render-annotations]"),
      original,
    );
    assert.equal(original.textContent, JSON.stringify({ plugin: malicious }));

    assert.equal(
      preview.apply({
        ...frame(3, [block("a", "updated")]),
        annotations: { plugin: "updated" },
      }),
      true,
    );
    assert.equal(
      root.querySelector("script[data-fleximark-render-annotations]")
        ?.textContent,
      JSON.stringify({ plugin: "updated" }),
    );
    assert.equal(
      preview.apply({ ...frame(4, [block("a", "updated")]), annotations: {} }),
      true,
    );
    assert.equal(
      root.querySelector("script[data-fleximark-render-annotations]"),
      null,
    );
  });

  test("highlights only changed blocks and cleans timers on replacement and disposal", () => {
    const originalSetTimeout = globalThis.setTimeout;
    const originalClearTimeout = globalThis.clearTimeout;
    const callbacks = new Map<number, () => void>();
    const cleared: number[] = [];
    const delays: number[] = [];
    let nextTimer = 0;
    globalThis.setTimeout = ((callback: () => void, delay: number) => {
      const timer = ++nextTimer;
      callbacks.set(timer, callback);
      delays.push(delay);
      return timer;
    }) as unknown as typeof setTimeout;
    globalThis.clearTimeout = ((timer: number) => {
      cleared.push(timer);
    }) as unknown as typeof clearTimeout;
    try {
      assert.equal(
        preview.apply(frame(1, [block("a", "one"), block("b", "same")])),
        true,
      );
      const unchanged = root.children[1];
      assert.equal(root.querySelector(".fade-highlight"), null);

      assert.equal(
        preview.apply(frame(2, [block("a", "two"), block("b", "same")])),
        true,
      );
      const changed = root.children[0] as HTMLElement;
      assert.equal(changed.classList.contains("fade-highlight"), true);
      assert.equal(root.children[1], unchanged);
      assert.equal(unchanged.classList.contains("fade-highlight"), false);

      assert.equal(
        preview.apply(frame(3, [block("a", "two"), block("b", "same")])),
        true,
      );
      assert.deepEqual(cleared, [1]);
      assert.equal(changed.classList.contains("fade-highlight"), false);
      callbacks.get(1)?.();
      assert.equal(root.querySelector(".fade-highlight"), null);

      assert.equal(
        preview.apply(frame(4, [block("a", "three"), block("b", "same")])),
        true,
      );
      const replacement = root.children[0] as HTMLElement;
      assert.equal(replacement.classList.contains("fade-highlight"), true);
      callbacks.get(2)?.();
      assert.equal(replacement.classList.contains("fade-highlight"), false);

      assert.equal(
        preview.apply(frame(5, [block("a", "four"), block("b", "same")])),
        true,
      );
      const disposed = root.children[0] as HTMLElement;
      assert.equal(disposed.classList.contains("fade-highlight"), true);
      preview.dispose();
      assert.deepEqual(delays, [1_000, 1_000, 1_000]);
      assert.deepEqual(cleared, [1, 3]);
      assert.equal(disposed.classList.contains("fade-highlight"), false);
      callbacks.get(3)?.();
      assert.equal(disposed.classList.contains("fade-highlight"), false);
    } finally {
      globalThis.setTimeout = originalSetTimeout;
      globalThis.clearTimeout = originalClearTimeout;
    }
  });

  test("rejects invalid frames atomically", () => {
    const invalidFrames = [
      frame(2, [
        block("b", "valid"),
        {
          id: "c",
          nodeIds: ["c"],
          html: '<script data-fleximark-node-id="c">alert(1)</script>',
        },
      ]),
      frame(2, [block("same", "one"), block("same", "two")]),
    ];
    for (const [index, invalid] of invalidFrames.entries()) {
      assert.equal(preview.apply(frame(1, [block("a", "safe")])), true);
      const html = root.innerHTML;
      assert.equal(preview.apply(invalid), false);
      assert.equal(root.innerHTML, html);
      assert.equal(preview.renderRevision, 1);
      if (index === 0) assert.equal(preview.apply(invalid), false);
      assert.equal(requested, 1);
      if (index + 1 < invalidFrames.length) {
        preview.dispose();
        root.replaceChildren();
        requested = 0;
        preview = new PreviewDocument(root, () => requested++);
      }
    }
  });

  test("accepts inert author attributes in rendered HTML", () => {
    const authored = {
      id: "a",
      nodeIds: ["a"],
      html: [
        '<details data-fleximark-node-id="a" id="note" class="fold"',
        ' style="color: rebeccapurple" data-topic="demo" open>',
        "<summary>Title</summary><p>Body</p></details>",
      ].join(""),
    };

    assert.equal(preview.apply(frame(1, [authored])), true);
    const details = root.querySelector("details");
    assert.equal(details?.id, "note");
    assert.equal(details?.className, "fold");
    assert.equal(details?.getAttribute("style"), "color: rebeccapurple");
    assert.equal(details?.dataset.topic, "demo");
    assert.equal(details?.textContent, "TitleBody");
  });

  test("ignores stale same-session frames without requesting another frame", () => {
    assert.equal(preview.apply(frame(2, [block("a", "new")])), true);
    assert.equal(preview.apply(frame(1, [block("a", "old")])), false);
    assert.equal(root.textContent, "new");
    assert.equal(requested, 0);
  });

  test("reuses retained asset URLs and revokes removed ones after commit", () => {
    const originalCreate = URL.createObjectURL;
    const originalRevoke = URL.revokeObjectURL;
    const created: string[] = [];
    const revoked: string[] = [];
    URL.createObjectURL = () => {
      const value = `blob:${created.length + 1}`;
      created.push(value);
      return value;
    };
    URL.revokeObjectURL = (value) => revoked.push(value);
    try {
      const firstAsset = asset("AQ==", "1".repeat(64));
      const secondAsset = asset("Ag==", "2".repeat(64));
      assert.equal(
        preview.apply({
          ...frame(1, [assetBlock("a", firstAsset.reference)]),
          assets: [firstAsset],
        }),
        true,
      );
      assert.equal(root.querySelector("img")?.getAttribute("src"), "blob:1");
      assert.equal(
        preview.apply({
          ...frame(2, [
            assetBlock("a", firstAsset.reference),
            assetBlock("b", secondAsset.reference),
          ]),
          assets: [firstAsset, secondAsset],
        }),
        true,
      );
      assert.deepEqual(created, ["blob:1", "blob:2"]);
      assert.deepEqual(revoked, []);
      assert.equal(
        preview.apply({ ...frame(3, [block("c", "plain")]), assets: [] }),
        true,
      );
      assert.deepEqual(revoked, ["blob:1", "blob:2"]);
    } finally {
      URL.createObjectURL = originalCreate;
      URL.revokeObjectURL = originalRevoke;
    }
  });

  test("rejects missing and malformed assets before changing the display", () => {
    assert.equal(preview.apply(frame(1, [block("a", "safe")])), true);
    const html = root.innerHTML;
    assert.equal(
      preview.apply(
        frame(2, [assetBlock("b", `fleximark-asset:${"4".repeat(64)}`)]),
      ),
      false,
    );
    assert.equal(
      preview.apply({
        ...frame(3, [block("c", "bad")]),
        assets: [
          {
            ...asset("not-base64", "5".repeat(64)),
            byteLength: 10,
          },
        ],
      }),
      false,
    );
    assert.equal(root.innerHTML, html);
    assert.equal(requested, 2);
  });

  test("applies validated theme CSS from the latest frame", () => {
    const style = { css: ":root{color:red}", fingerprint: "3".repeat(64) };
    assert.equal(
      preview.apply({ ...frame(1, [block("a", "one")]), style }),
      true,
    );
    assert.equal(
      document.querySelector("style[data-fleximark-theme]")?.textContent,
      style.css,
    );
  });

  test("renders Mermaid and drops an asynchronous result after disposal", async () => {
    assert.equal(
      preview.apply(specialFrame("mermaid", "graph TD; A-->B")),
      true,
    );
    const pending = deferred<{ svg: string }>();
    const runtimes = inertRuntimes();
    runtimes.mermaid.render = () => pending.promise;
    const enhancer = new PreviewEnhancer(runtimes);
    const rendering = enhancer.render(root);
    enhancer.dispose();
    pending.resolve({ svg: '<svg data-stale="true"></svg>' });
    await rendering;
    assert.equal(root.querySelector("svg[data-stale=true]"), null);

    const active = new PreviewEnhancer(inertRuntimes());
    await active.render(root);
    assert.ok(root.querySelector("[data-fleximark-output] svg"));
    active.dispose();
  });

  test("renders math with visual HTML and MathML output", async () => {
    assert.equal(
      preview.apply({
        ...specialFrame("math", ""),
        blocks: [
          {
            id: "special",
            nodeIds: ["special"],
            html: '<div data-fleximark-node-id="special" data-fleximark-kind="math">x^2</div>',
          },
        ],
      }),
      true,
    );
    const { previewRuntimes } =
      await import("../web/preview-client/runtimes.mjs");
    const enhancer = new PreviewEnhancer(previewRuntimes);
    await enhancer.render(root);
    assert.ok(root.querySelector("[data-fleximark-output] math msup"));
    assert.ok(root.querySelector("[data-fleximark-output] .katex-html"));
    enhancer.dispose();
  });

  test("renders Japanese text in math without a KaTeX error", async () => {
    assert.equal(
      preview.apply({
        ...specialFrame("math", ""),
        blocks: [
          {
            id: "special",
            nodeIds: ["special"],
            html: '<div data-fleximark-node-id="special" data-fleximark-kind="math">\\scriptsize{※ 畳み込みは可換}</div>',
          },
        ],
      }),
      true,
    );
    const { previewRuntimes } =
      await import("../web/preview-client/runtimes.mjs");
    const enhancer = new PreviewEnhancer(previewRuntimes);
    await enhancer.render(root);
    const output = root.querySelector<HTMLElement>("[data-fleximark-output]");
    assert.ok(output);
    assert.equal(output.querySelector(".katex-error"), null);
    assert.match(output.textContent ?? "", /畳み込みは可換/);
    enhancer.dispose();
  });

  test("keeps inline math output inline", async () => {
    assert.equal(
      preview.apply(
        frame(1, [
          {
            id: "paragraph",
            nodeIds: ["paragraph"],
            html: '<p data-fleximark-node-id="paragraph">before <span data-fleximark-kind="math">x^2</span> after</p>',
          },
        ]),
      ),
      true,
    );
    let displayMode: boolean | undefined;
    const runtimes = inertRuntimes();
    runtimes.math.render = (_source, target, options) => {
      displayMode = options.displayMode;
      target.textContent = "rendered";
    };
    const enhancer = new PreviewEnhancer(runtimes);
    await enhancer.render(root);

    const math = root.querySelector<HTMLElement>("[data-fleximark-kind=math]");
    const output = math?.querySelector<HTMLElement>(
      ":scope > [data-fleximark-output]",
    );
    assert.equal(math?.tagName, "SPAN");
    assert.equal(output?.tagName, "SPAN");
    assert.equal(displayMode, false);
    assert.equal(math?.querySelector("div"), null);
    assert.equal(math?.parentElement?.firstChild?.textContent, "before ");
    assert.equal(math?.parentElement?.lastChild?.textContent, " after");
    enhancer.dispose();
  });

  test("shows a YouTube thumbnail and loads a referrer-enabled player", async () => {
    assert.equal(
      preview.apply(
        frame(1, [
          {
            id: "youtube",
            nodeIds: ["youtube"],
            html: '<div data-fleximark-node-id="youtube" data-fleximark-kind="youtube" data-source="https://youtu.be/M7lc1UVf-VE"></div>',
          },
        ]),
      ),
      true,
    );
    const enhancer = new PreviewEnhancer(inertRuntimes());
    await enhancer.render(root);

    const button = root.querySelector<HTMLButtonElement>(
      "button.youtube-placeholder",
    );
    const thumbnail = button?.querySelector<HTMLImageElement>("img");
    assert.equal(button?.getAttribute("aria-label"), "Play YouTube video");
    assert.equal(
      thumbnail?.src,
      "https://i.ytimg.com/vi/M7lc1UVf-VE/hqdefault.jpg",
    );
    assert.equal(thumbnail?.alt, "");

    button?.click();
    const frameElement = root.querySelector<HTMLIFrameElement>("iframe");
    assert.equal(
      frameElement?.src,
      "https://www.youtube-nocookie.com/embed/M7lc1UVf-VE",
    );
    assert.equal(
      frameElement?.getAttribute("referrerpolicy"),
      "strict-origin-when-cross-origin",
    );
    assert.match(frameElement?.getAttribute("allow") ?? "", /autoplay/);
    assert.equal(frameElement?.hasAttribute("allowfullscreen"), true);
    enhancer.dispose();
  });

  test("renders ABC notation into SVG output", async () => {
    assert.equal(preview.apply(specialFrame("abc", "X:1\nK:C\nC")), true);
    const { previewRuntimes } =
      await import("../web/preview-client/runtimes.mjs");
    const enhancer = new PreviewEnhancer(previewRuntimes);
    await enhancer.render(root);
    const svg = root.querySelector("[data-fleximark-output] svg");
    assert.ok(
      svg,
      root.querySelector<HTMLElement>("[data-fleximark-kind=abc]")?.dataset
        .fleximarkRenderError ?? root.innerHTML,
    );
    enhancer.dispose();
  });

  test("renders ABC audio as an accessible playback bar", async () => {
    assert.equal(preview.apply(specialFrame("abc", "X:1\nK:C\nC")), true);
    const runtimes = inertRuntimes();
    runtimes.abc.render = (target) => {
      target.innerHTML = "<svg></svg>";
      return [{}];
    };
    runtimes.abc.supportsAudio = () => true;
    runtimes.abc.createTiming = () => ({
      start: () => undefined,
      pause: () => undefined,
      stop: () => undefined,
      reset: () => undefined,
      setProgress: () => undefined,
      currentMillisecond: () => 0,
      duration: () => 65_000,
    });
    const enhancer = new PreviewEnhancer(runtimes);
    await enhancer.render(root);

    const player = root.querySelector<HTMLElement>(
      "[data-fleximark-audio=player]",
    );
    const toggle = player?.querySelector<HTMLButtonElement>("button");
    const progress = player?.querySelector<HTMLInputElement>(
      'input[type="range"]',
    );
    assert.equal(player?.getAttribute("aria-label"), "ABC playback");
    assert.equal(
      player?.previousElementSibling?.getAttribute("data-fleximark-output"),
      "true",
    );
    assert.equal(toggle?.getAttribute("aria-label"), "Play");
    assert.equal(toggle?.textContent, "");
    assert.equal(toggle?.dataset.fleximarkAudioState, "play");
    assert.equal(progress?.max, "65");
    assert.equal(progress?.getAttribute("aria-label"), "Playback position");
    assert.equal(
      player?.querySelector(".fleximark-audio-time")?.textContent,
      "0:00 / 1:05",
    );
    enhancer.dispose();
  });

  test("keeps typed tabs accessible across an unrelated reused block", async () => {
    const tabs = {
      id: "tabs",
      nodeIds: ["tabs", "one", "two"],
      html: '<section data-fleximark-node-id="tabs" data-fleximark-kind="tabs"><section data-fleximark-node-id="one" data-fleximark-kind="tab" data-tab-label="One">first</section><section data-fleximark-node-id="two" data-fleximark-kind="tab" data-tab-label="Two">second</section></section>',
    };
    assert.equal(
      preview.apply(frame(1, [tabs, block("outside", "before")])),
      true,
    );
    const enhancer = new PreviewEnhancer(inertRuntimes());
    await enhancer.render(root);
    const originalTabs = root.firstElementChild;
    const buttons = [...root.querySelectorAll<HTMLButtonElement>("[role=tab]")];
    buttons[1].click();
    assert.equal(buttons[1].getAttribute("aria-selected"), "true");

    assert.equal(
      preview.apply(frame(2, [tabs, block("outside", "after")])),
      true,
    );
    assert.equal(root.firstElementChild, originalTabs);
    await enhancer.render(root);
    assert.equal(
      root.querySelectorAll("[role=tab]")[1]?.getAttribute("aria-selected"),
      "true",
    );
    enhancer.dispose();
  });

  test("accepts authenticated frame host messages and rejects malformed ones", () => {
    const message = {
      type: "previewFrame",
      messageToken: "secret",
      frame: frame(1, [block("a", "one")]),
    };
    assert.equal(isPreviewHostMessage(message), true);
    assert.equal(isPreviewHostMessageEvent(message, "secret"), true);
    assert.equal(isPreviewHostMessageEvent(message, "other"), false);
    assert.equal(
      isInvalidAuthenticatedFrameMessage(
        { ...message, frame: { ...message.frame, blocks: "invalid" } },
        "secret",
      ),
      true,
    );
  });

  test("host requests one fresh frame when a frame cannot be applied", () => {
    let framesRequested = 0;
    const host = new PreviewHost(
      root,
      () => framesRequested++,
      () => undefined,
    );
    host.apply([frame(1, [block("a", "one")])]);
    host.apply([
      {
        ...frame(2, []),
        blocks: [
          {
            id: "bad",
            nodeIds: ["bad"],
            html: '<iframe data-fleximark-node-id="bad"></iframe>',
          },
        ],
      },
    ]);
    assert.equal(framesRequested, 1);
    assert.equal(root.textContent, "one");
    host.dispose();
  });

  test("forwards editor navigation only for the current revision", () => {
    const event = {
      type: "selectNode" as const,
      previewSessionId: "preview",
      renderRevision: 3,
      nodeId: "a",
    };
    assert.equal(shouldForwardEditorNavigation(event, "preview", 3), true);
    assert.equal(shouldForwardEditorNavigation(event, "preview", 2), false);
  });

  test("maps selection navigation to the nearest known node", () => {
    root.innerHTML = [
      '<p data-fleximark-node-id="a">one</p>',
      '<p data-fleximark-node-id="b">two</p>',
    ].join("");
    const navigation = new PreviewNavigation(root, () => undefined);
    navigation.setKnownIds(["a", "b"], [entry("a", 0), entry("b", 2)]);
    navigation.receive({
      type: "selection",
      nodeIds: ["b"],
      activePosition: null,
    });
    assert.equal(
      (root.children[1] as HTMLElement).dataset.fleximarkSelected,
      "true",
    );
    navigation.dispose();
  });
}

function frame(
  renderRevision: number,
  blocks: RenderFrame["blocks"],
): RenderFrame {
  return {
    previewSessionId: "preview",
    documentVersion: renderRevision,
    renderRevision,
    rendererFingerprint: "a".repeat(64),
    style: null,
    assets: [],
    blocks,
    navigation: blocks.map(({ id }, line) => entry(id, line)),
    annotations: {},
  };
}

function block(id: string, text: string): RenderFrame["blocks"][number] {
  return {
    id,
    nodeIds: [id],
    html: `<p data-fleximark-node-id="${id}">${text}</p>`,
  };
}

function assetBlock(
  id: string,
  reference: string,
): RenderFrame["blocks"][number] {
  return {
    id,
    nodeIds: [id],
    html: `<p data-fleximark-node-id="${id}"><img src="${reference}"></p>`,
  };
}

function entry(nodeId: string, line: number) {
  return {
    nodeId,
    sourceRange: {
      byteStart: line,
      byteEnd: line + 1,
      start: { line, character: 0, encoding: "utf8" as const },
      end: { line, character: 1, encoding: "utf8" as const },
    },
    depth: 0,
  };
}

function asset(data: string, contentHash: string): RenderAsset {
  return {
    reference: `fleximark-asset:${contentHash}`,
    mediaType: "image/png",
    contentHash,
    byteLength: 1,
    data,
  };
}

function specialFrame(
  kind: "mermaid" | "abc" | "math",
  source: string,
): RenderFrame {
  return frame(1, [
    {
      id: "special",
      nodeIds: ["special"],
      html: `<div data-fleximark-node-id="special" data-fleximark-kind="${kind}"><script type="application/json">${JSON.stringify(source).replaceAll("<", "\\u003c")}</script></div>`,
    },
  ]);
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
      createTiming: () => ({
        start: () => undefined,
        pause: () => undefined,
        stop: () => undefined,
        reset: () => undefined,
        setProgress: () => undefined,
        currentMillisecond: () => 0,
        duration: () => 0,
      }),
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
