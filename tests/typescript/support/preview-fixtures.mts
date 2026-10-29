import type { PreviewRuntimes } from "@fleximark/preview/enhance";
import type { NavigationEntry, RenderFrame } from "@fleximark/preview/index";

export function frame(
  renderRevision: number,
  blocks: RenderFrame["blocks"],
  overrides: Partial<RenderFrame> = {},
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
    ...overrides,
  };
}

export function block(id: string, text: string): RenderFrame["blocks"][number] {
  return {
    id,
    nodeIds: [id],
    html: `<p data-fleximark-node-id="${id}">${text}</p>`,
  };
}

export function entry(
  nodeId: string,
  line: number,
  width = 1,
): NavigationEntry {
  return {
    nodeId,
    sourceRange: {
      byteStart: line,
      byteEnd: line + width,
      start: { line, character: 0, encoding: "utf8" as const },
      end: { line, character: width, encoding: "utf8" as const },
    },
    depth: 0,
  };
}

export function textFrame(revision: number, text: string): RenderFrame {
  return frame(revision, [block("paragraph", text)], {
    navigation: [entry("paragraph", 0, text.length)],
  });
}

export function specialFrame(
  kind: "mermaid" | "abc" | "math",
  source: string,
  overrides: Partial<RenderFrame> = {},
  id = "special",
): RenderFrame {
  return frame(
    1,
    [
      {
        id,
        nodeIds: [id],
        html: `<div data-fleximark-node-id="${id}" data-fleximark-kind="${kind}"><script type="application/json">${JSON.stringify(source).replaceAll("<", "\\u003c")}</script></div>`,
      },
    ],
    overrides,
  );
}

export function inertRuntimes(): PreviewRuntimes {
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
