import * as assert from "node:assert/strict";
import { readFileSync } from "node:fs";

import {
  PreviewEnhancer,
  type PreviewRuntimes,
} from "../../web/preview/enhance.mjs";
import type { PreviewDocument, RenderFrame } from "../../web/preview/index.mjs";
import { PreviewNavigation } from "../../web/preview/navigation.mjs";

export function registerExtendedPreviewTests(
  current: () => { root: HTMLElement; preview: PreviewDocument },
): void {
  test("enlarges only the Mermaid diagram and restores it when closed", async () => {
    const { root, preview } = current();
    assert.equal(
      preview.apply(specialFrame("mermaid", "graph TD; A-->B")),
      true,
    );
    const runtimes = inertRuntimes();
    runtimes.mermaid.render = async () => ({
      svg: '<svg id="diagram" viewBox="0 0 1200 600" style="max-width: 1200px"><text>Diagram</text></svg>',
    });
    const enhancer = new PreviewEnhancer(runtimes);
    await enhancer.render(root);
    const enlarge = root.querySelector<HTMLButtonElement>(
      ".fleximark-mermaid-enlarge",
    );
    const diagram = root.querySelector<SVGSVGElement>("svg");
    assert.ok(enlarge);
    assert.ok(diagram);
    const originalStyle = diagram.getAttribute("style");
    const createElement = document.createElement.bind(document);
    document.createElement = ((tag: string) => {
      const element = createElement(tag);
      if (tag === "dialog")
        Object.assign(element, {
          showModal() {
            element.setAttribute("open", "");
          },
          close() {
            element.removeAttribute("open");
            element.dispatchEvent(new Event("close"));
          },
        });
      return element;
    }) as typeof document.createElement;
    try {
      enlarge.click();
      const dialog = document.querySelector<HTMLDialogElement>(
        ".fleximark-mermaid-dialog",
      );
      assert.ok(dialog);
      assert.equal(dialog.hasAttribute("open"), true);
      assert.equal(
        dialog.getAttribute("aria-label"),
        "Enlarged Mermaid diagram",
      );
      assert.equal(dialog.querySelector("svg"), diagram);
      assert.equal(document.querySelectorAll("#diagram").length, 1);
      assert.equal(dialog.querySelector("script"), null);
      assert.equal(diagram.style.width, "1200px");
      const controls = [
        ...dialog.querySelectorAll<HTMLButtonElement>("button"),
      ];
      controls.find((control) => control.textContent === "Zoom in")?.click();
      assert.equal(diagram.style.width, "1500px");
      assert.equal(dialog.querySelector("output")?.textContent, "125%");
      controls.find((control) => control.textContent === "Reset zoom")?.click();
      assert.equal(diagram.style.width, "1200px");
      controls.find((control) => control.textContent === "Close")?.click();
      assert.equal(document.querySelector("dialog"), null);
      assert.equal(root.querySelector("svg"), diagram);
      assert.equal(diagram.getAttribute("style"), originalStyle);

      await enhancer.render(root);
      assert.equal(
        root.querySelectorAll(".fleximark-mermaid-enlarge").length,
        1,
      );
      enlarge.click();
      document.querySelector("dialog")?.dispatchEvent(new Event("cancel"));
      assert.equal(document.querySelector("dialog"), null);
      assert.equal(root.querySelector("svg"), diagram);

      enlarge.click();
      assert.equal(
        preview.apply(frame(2, [block("replacement", "updated")])),
        true,
      );
      await enhancer.render(root);
      assert.equal(document.querySelector("dialog"), null);
      assert.equal(root.textContent, "updated");
    } finally {
      enhancer.dispose();
      document.createElement = createElement;
    }
  });

  test("removes an enlarged Mermaid view when the enhancer is disposed", async () => {
    const { root, preview } = current();
    assert.equal(
      preview.apply(specialFrame("mermaid", "graph TD; A-->B")),
      true,
    );
    const enhancer = new PreviewEnhancer(inertRuntimes());
    await enhancer.render(root);
    const createElement = document.createElement.bind(document);
    document.createElement = ((tag: string) => {
      const element = createElement(tag);
      if (tag === "dialog")
        Object.assign(element, {
          showModal: () => element.setAttribute("open", ""),
          close: () => element.removeAttribute("open"),
        });
      return element;
    }) as typeof document.createElement;
    try {
      root
        .querySelector<HTMLButtonElement>(".fleximark-mermaid-enlarge")
        ?.click();
      assert.ok(document.querySelector("dialog"));
      enhancer.dispose();
      assert.equal(document.querySelector("dialog"), null);
      assert.equal(root.querySelector(".fleximark-mermaid-enlarge"), null);
      assert.ok(root.querySelector("svg"));
    } finally {
      enhancer.dispose();
      document.createElement = createElement;
    }
  });

  test("scrolls to the visible descendant in a Rust-rendered nested container", async () => {
    const { root, preview } = current();
    const rendered = JSON.parse(
      readFileSync(
        "tests/fixtures/protocol/rust-navigation-frame.json",
        "utf8",
      ),
    ) as RenderFrame;
    assert.equal(preview.apply(rendered), true);
    const paragraph = [...root.querySelectorAll<HTMLElement>("p")].find(
      (node) => node.textContent?.startsWith("Nested paragraph 40."),
    );
    assert.ok(paragraph);
    const id = paragraph.dataset.fleximarkNodeId;
    assert.ok(id);
    Object.defineProperty(window, "innerHeight", {
      configurable: true,
      value: 720,
    });
    for (const node of root.querySelectorAll<HTMLElement>(
      "[data-fleximark-node-id]",
    ))
      node.getBoundingClientRect = () =>
        ({ top: -40, bottom: -20, height: 20 }) as DOMRect;
    paragraph.getBoundingClientRect = () =>
      ({ top: -5, bottom: 25, height: 30 }) as DOMRect;
    let parent = paragraph.parentElement;
    while (parent && parent !== root) {
      parent.getBoundingClientRect = () =>
        ({ top: -1000, bottom: 700, height: 1700 }) as DOMRect;
      parent = parent.parentElement;
    }
    const hidden = document.createElement("span");
    hidden.dataset.fleximarkNodeId = "raw-html-boundary";
    hidden.hidden = true;
    hidden.getBoundingClientRect = () =>
      ({ top: 0, bottom: 0, height: 0 }) as DOMRect;
    root.prepend(hidden);
    const events: unknown[] = [];
    const navigation = new PreviewNavigation(root, (event) =>
      events.push(event),
    );
    navigation.setKnownIds(
      ["raw-html-boundary", ...rendered.navigation.map(({ nodeId }) => nodeId)],
      rendered.navigation,
    );
    try {
      window.dispatchEvent(new Event("scroll"));
      await new Promise((resolve) => setTimeout(resolve, 120));
      assert.deepEqual(events, [{ type: "revealNode", nodeId: id }]);
      const source = rendered.navigation.find((entry) => entry.nodeId === id);
      assert.equal(source?.sourceRange.start.line, 82);
    } finally {
      navigation.dispose();
    }
  });

  test("a no-op editor reveal does not suppress the next user scroll", async () => {
    const { root } = current();
    root.innerHTML = '<p data-fleximark-node-id="a">one</p>';
    const paragraph = root.firstElementChild as HTMLElement;
    paragraph.scrollIntoView = () => undefined;
    paragraph.getBoundingClientRect = () =>
      ({ top: 0, bottom: 20, height: 20 }) as DOMRect;
    Object.defineProperties(window, {
      innerHeight: { configurable: true, value: 720 },
      scrollY: { configurable: true, writable: true, value: 0 },
    });
    const events: unknown[] = [];
    const navigation = new PreviewNavigation(root, (event) =>
      events.push(event),
    );
    navigation.setKnownIds(["a"]);
    try {
      navigation.receive({ type: "viewport", nodeId: "a" });
      window.scrollY = 40;
      window.dispatchEvent(new Event("scroll"));
      await new Promise((resolve) => setTimeout(resolve, 120));
      assert.deepEqual(events, [{ type: "revealNode", nodeId: "a" }]);
    } finally {
      navigation.dispose();
    }
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
