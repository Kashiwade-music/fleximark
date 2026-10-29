import { PreviewEnhancer } from "@fleximark/preview/enhance";
import type { PreviewDocument, RenderFrame } from "@fleximark/preview/index";
import { PreviewNavigation } from "@fleximark/preview/navigation";
import * as assert from "node:assert/strict";
import { readFileSync } from "node:fs";

import { replaceProperties } from "../support/dom-fixtures.mjs";
import {
  block,
  entry,
  frame,
  inertRuntimes,
  specialFrame,
} from "../support/preview-fixtures.mjs";

export function registerExtendedPreviewTests(
  current: () => { root: HTMLElement; preview: PreviewDocument },
): void {
  test("ABC owns cursor highlighting and clears it when removed", async () => {
    const { root, preview } = current();
    const runtimes = inertRuntimes();
    let note: Element | undefined;
    runtimes.abc.render = (output) => {
      output.innerHTML = "<svg><g></g></svg>";
      note = output.querySelector("g") ?? undefined;
      return [
        {
          getSelectableArray: () => [
            { absEl: { abcelem: { startChar: 8, endChar: 9 } }, svgEl: note },
          ],
        },
      ];
    };
    const enhancer = new PreviewEnhancer(runtimes);
    const navigation = new PreviewNavigation(root, () => undefined);
    const initial = specialFrame("abc", "X:1\nK:C\nC");
    try {
      assert.equal(preview.apply(initial), true);
      await enhancer.render(root);
      // Navigation uses the source captured by the ABC enhancer, not a second payload parse.
      const payload = root.querySelector("script");
      assert.ok(payload);
      payload.textContent = "invalid JSON after enhancement";
      navigation.setKnownIds(["special"], initial.navigation);
      navigation.receive({
        previewSessionId: "preview",
        renderRevision: 1,
        type: "selection",
        nodeIds: ["special"],
        activePosition: { line: 3, character: 0 },
      });
      assert.ok(note?.classList.contains("fade-highlight-cursor-abc"));
      assert.equal(
        preview.apply(frame(2, [block("next", "replacement")])),
        true,
      );
      await enhancer.render(root);
      assert.equal(
        note?.classList.contains("fade-highlight-cursor-abc"),
        false,
      );
    } finally {
      navigation.dispose();
      enhancer.dispose();
    }
  });

  test("disposing ABC during synth initialization releases it and suppresses playback", async () => {
    const { root, preview } = current();
    const runtimes = inertRuntimes();
    runtimes.abc.render = () => [{}];
    runtimes.abc.supportsAudio = () => true;
    const timing = runtimes.abc.createTiming(
      {},
      { beat: () => undefined, event: () => undefined },
    );
    runtimes.abc.createTiming = () => ({ ...timing, duration: () => 1000 });
    let initialized: (() => void) | undefined;
    let stops = 0;
    let primes = 0;
    let starts = 0;
    runtimes.abc.createSynth = () => ({
      init: () =>
        new Promise<void>((resolve) => {
          initialized = resolve;
        }),
      prime: async () => {
        primes++;
      },
      start: () => {
        starts++;
      },
      stop: () => {
        stops++;
      },
    });
    const enhancer = new PreviewEnhancer(runtimes);
    assert.equal(preview.apply(specialFrame("abc", "X:1\nK:C\nC")), true);
    await enhancer.render(root);
    const button = root.querySelector<HTMLButtonElement>(
      ".fleximark-audio-toggle",
    );
    assert.ok(button);
    button.click();
    enhancer.dispose();
    assert.equal(stops, 1);
    initialized?.();
    await new Promise<void>((resolve) => setImmediate(resolve));
    assert.equal(primes, 0);
    assert.equal(starts, 0);
    button.click();
    assert.equal(stops, 1);
  });

  test("removed asynchronous Mermaid output cannot enhance its replacement", async () => {
    const { root, preview } = current();
    const runtimes = inertRuntimes();
    let complete: ((value: { svg: string }) => void) | undefined;
    runtimes.mermaid.render = () =>
      new Promise((resolve) => {
        complete = resolve;
      });
    const enhancer = new PreviewEnhancer(runtimes);
    try {
      assert.equal(
        preview.apply(specialFrame("mermaid", "graph TD; A-->B")),
        true,
      );
      const pending = enhancer.render(root);
      assert.equal(
        preview.apply(frame(2, [block("next", "replacement")])),
        true,
      );
      await enhancer.render(root);
      complete?.({ svg: "<svg><text>stale</text></svg>" });
      await pending;
      assert.equal(root.textContent, "replacement");
      assert.equal(root.querySelector(".fleximark-mermaid-enlarge"), null);
      assert.equal(document.querySelector("dialog"), null);
    } finally {
      enhancer.dispose();
    }
  });

  test("enhances unchanged Mermaid inside a rebuilt parent after a sibling edit", async () => {
    const { root, preview } = current();
    const runtimes = inertRuntimes();
    let renders = 0;
    runtimes.mermaid.render = async () => ({
      svg: `<svg><text>diagram ${++renders}</text></svg>`,
    });
    const enhancer = new PreviewEnhancer(runtimes);
    const parentFrame = (revision: number, text: string): RenderFrame => ({
      ...frame(revision, []),
      blocks: [
        {
          id: "parent",
          nodeIds: ["parent", "paragraph", "special"],
          html: `<div data-fleximark-node-id="parent"><p data-fleximark-node-id="paragraph">${text}</p>${specialFrame("mermaid", "graph TD; A-->B").blocks[0].html}</div>`,
        },
      ],
      navigation: [
        entry("parent", 0),
        entry("paragraph", 1),
        entry("special", 2),
      ],
    });
    try {
      assert.equal(preview.apply(parentFrame(1, "before")), true);
      await enhancer.render(root);
      const first = root.querySelector("svg");
      assert.ok(first);
      assert.equal(preview.apply(parentFrame(2, "after")), true);
      await enhancer.render(root);
      assert.equal(root.querySelector("p")?.textContent, "after");
      assert.ok(root.querySelector("svg"));
      assert.notEqual(root.querySelector("svg"), first);
      assert.equal(renders, 2);
      await enhancer.render(root);
      assert.equal(renders, 2, "retained DOM is enhanced only once");
      assert.equal(
        preview.apply({
          ...parentFrame(3, "after"),
          rendererFingerprint: "b".repeat(64),
        }),
        true,
      );
      await enhancer.render(root);
      assert.equal(renders, 3);
      assert.equal(
        preview.apply({
          ...parentFrame(1, "after"),
          previewSessionId: "replacement",
        }),
        true,
      );
      await enhancer.render(root);
      assert.equal(renders, 4, "new session DOM is always enhanced");
    } finally {
      enhancer.dispose();
    }
  });

  test("retained asynchronous Mermaid completion survives another frame", async () => {
    const { root, preview } = current();
    const runtimes = inertRuntimes();
    let complete: ((value: { svg: string }) => void) | undefined;
    let renders = 0;
    runtimes.mermaid.render = () => {
      renders++;
      return new Promise((resolve) => {
        complete = resolve;
      });
    };
    const enhancer = new PreviewEnhancer(runtimes);
    const initial = specialFrame("mermaid", "graph TD; A-->B");
    try {
      assert.equal(preview.apply(initial), true);
      const pending = enhancer.render(root);
      assert.equal(
        preview.apply({ ...initial, renderRevision: 2, documentVersion: 2 }),
        true,
      );
      await enhancer.render(root);
      complete?.({ svg: "<svg><text>completed</text></svg>" });
      await pending;
      assert.equal(renders, 1);
      assert.equal(root.querySelector("svg")?.textContent, "completed");
      assert.ok(root.querySelector(".fleximark-mermaid-enlarge"));
    } finally {
      enhancer.dispose();
    }
  });

  test("retains pending ABC playback across frames and stops removed audio", async () => {
    const { root, preview } = current();
    const animation = replaceProperties(globalThis, {
      requestAnimationFrame: { configurable: true, value: () => 1 },
      cancelAnimationFrame: { configurable: true, value: () => undefined },
    });
    const runtimes = inertRuntimes();
    runtimes.abc.render = () => [{}];
    runtimes.abc.supportsAudio = () => true;
    const timing = runtimes.abc.createTiming(
      {},
      { beat: () => undefined, event: () => undefined },
    );
    runtimes.abc.createTiming = () => ({ ...timing, duration: () => 1000 });
    let initialized: (() => void) | undefined;
    let started = 0;
    let stopped = 0;
    runtimes.abc.createSynth = () => ({
      init: () =>
        new Promise<void>((resolve) => {
          initialized = resolve;
        }),
      prime: async () => undefined,
      start: () => {
        started++;
      },
      stop: () => {
        stopped++;
      },
    });
    const enhancer = new PreviewEnhancer(runtimes);
    const initial = specialFrame("abc", "X:1\nK:C\nC");
    try {
      assert.equal(preview.apply(initial), true);
      await enhancer.render(root);
      const button = root.querySelector<HTMLButtonElement>(
        ".fleximark-audio-toggle",
      );
      assert.ok(button);
      button.click();
      assert.equal(
        preview.apply({ ...initial, renderRevision: 2, documentVersion: 2 }),
        true,
      );
      await enhancer.render(root);
      assert.equal(root.querySelector(".fleximark-audio-toggle"), button);
      initialized?.();
      await new Promise<void>((resolve) => setImmediate(resolve));
      assert.equal(started, 1);
      assert.equal(stopped, 0);
      assert.equal(button.dataset.fleximarkAudioState, "pause");
      assert.equal(
        preview.apply(frame(3, [block("replacement", "changed")])),
        true,
      );
      await enhancer.render(root);
      assert.equal(stopped, 1);
    } finally {
      enhancer.dispose();
      animation.dispose();
    }
  });

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
      navigation.receive({
        type: "viewport",
        nodeId: "a",
        previewSessionId: "preview",
        renderRevision: 1,
      });
      window.scrollY = 40;
      window.dispatchEvent(new Event("scroll"));
      await new Promise((resolve) => setTimeout(resolve, 120));
      assert.deepEqual(events, [{ type: "revealNode", nodeId: "a" }]);
    } finally {
      navigation.dispose();
    }
  });
}
