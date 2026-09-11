import type { NavigationEntry } from "./index.mjs";

export type PreviewNavigationUpdate =
  | {
      type: "selection";
      nodeIds: string[];
      activePosition?: { line: number; character: number } | null;
    }
  | {
      type: "viewport";
      nodeId: string;
    };

export type PreviewNavigationEvent = PreviewNavigationUpdate & {
  previewSessionId: string;
  renderRevision: number;
};

export type PreviewNodeEvent =
  | { type: "selectNode"; nodeId: string }
  | { type: "revealNode"; nodeId: string };

export type EditorNavigationEvent = PreviewNodeEvent & {
  previewSessionId: string;
  renderRevision: number;
};

export class PreviewNavigation {
  readonly #root: HTMLElement;
  readonly #send: (event: PreviewNodeEvent) => void;
  readonly #onClick: (event: Event) => void;
  readonly #onScroll: () => void;
  #knownIds = new Set<string>();
  #navigation = new Map<string, NavigationEntry>();
  readonly #abcHighlightTimers = new Map<
    Element,
    ReturnType<typeof setTimeout>
  >();
  #scrollTimer?: ReturnType<typeof setTimeout>;
  #suppressScroll = false;

  constructor(root: HTMLElement, send: (event: PreviewNodeEvent) => void) {
    this.#root = root;
    this.#send = send;
    this.#onClick = (event) => {
      const target = event.target;
      const node =
        target instanceof Element
          ? target.closest<HTMLElement>("[data-fleximark-node-id]")
          : null;
      const nodeId = node?.dataset.fleximarkNodeId;
      if (nodeId && this.#knownIds.has(nodeId))
        this.#send({ type: "selectNode", nodeId });
    };
    this.#onScroll = () => {
      if (this.#suppressScroll) {
        this.#suppressScroll = false;
        return;
      }
      if (this.#scrollTimer) clearTimeout(this.#scrollTimer);
      this.#scrollTimer = setTimeout(() => {
        const node = [
          ...this.#root.querySelectorAll<HTMLElement>(
            "[data-fleximark-node-id]",
          ),
        ].find((element) => element.getBoundingClientRect().bottom >= 0);
        const nodeId = node?.dataset.fleximarkNodeId;
        if (nodeId && this.#knownIds.has(nodeId))
          this.#send({ type: "revealNode", nodeId });
      }, 100);
    };
    root.addEventListener("click", this.#onClick);
    window.addEventListener("scroll", this.#onScroll, { passive: true });
  }

  setKnownIds(
    ids: Iterable<string>,
    navigation: readonly NavigationEntry[] = [],
  ): void {
    this.#knownIds = new Set(ids);
    this.#navigation = new Map(
      navigation.map((entry) => [entry.nodeId, entry]),
    );
  }

  receive(event: PreviewNavigationUpdate): void {
    if (event.type === "selection") {
      const selected = new Set(event.nodeIds);
      for (const node of this.#root.querySelectorAll<HTMLElement>(
        "[data-fleximark-node-id]",
      )) {
        if (selected.has(node.dataset.fleximarkNodeId ?? ""))
          node.dataset.fleximarkSelected = "true";
        else delete node.dataset.fleximarkSelected;
      }
      this.#highlightAbcCursor(event);
      return;
    }
    if (!this.#knownIds.has(event.nodeId)) return;
    const node = [
      ...this.#root.querySelectorAll<HTMLElement>("[data-fleximark-node-id]"),
    ].find((element) => element.dataset.fleximarkNodeId === event.nodeId);
    if (node) {
      this.#suppressScroll = true;
      node.scrollIntoView({ block: "start" });
    }
  }

  dispose(): void {
    this.#root.removeEventListener("click", this.#onClick);
    window.removeEventListener("scroll", this.#onScroll);
    if (this.#scrollTimer) clearTimeout(this.#scrollTimer);
    for (const [element, timer] of this.#abcHighlightTimers) {
      clearTimeout(timer);
      element.classList.remove("fade-highlight-cursor-abc");
    }
    this.#abcHighlightTimers.clear();
  }

  #highlightAbcCursor(
    event: Extract<PreviewNavigationUpdate, { type: "selection" }>,
  ): void {
    const position = event.activePosition;
    if (!position) return;
    const blocks = [
      ...this.#root.querySelectorAll<HTMLElement>(
        '[data-fleximark-kind="abc"][data-fleximark-node-id]',
      ),
    ];
    const block = blocks.find((candidate) =>
      event.nodeIds.includes(candidate.dataset.fleximarkNodeId ?? ""),
    );
    const nodeId = block?.dataset.fleximarkNodeId;
    const navigation = nodeId ? this.#navigation.get(nodeId) : undefined;
    if (!block || !navigation) return;
    const source = [...block.children].find(
      (child) =>
        child instanceof HTMLScriptElement && child.type === "application/json",
    )?.textContent;
    if (source === undefined) return;
    let abcSource: unknown;
    try {
      abcSource = JSON.parse(source);
    } catch {
      return;
    }
    if (typeof abcSource !== "string") return;
    const relativeLine = position.line - navigation.sourceRange.start.line - 1;
    const offset = sourceOffset(abcSource, relativeLine, position.character);
    if (offset === undefined) return;
    for (const element of block.querySelectorAll<Element>(
      "[data-relative-char-number-start][data-relative-char-number-end]",
    )) {
      const start = Number(
        element.getAttribute("data-relative-char-number-start"),
      );
      const end = Number(element.getAttribute("data-relative-char-number-end"));
      if (Number.isSafeInteger(start) && offset >= start && offset < end)
        this.#restartAbcHighlight(element);
    }
  }

  #restartAbcHighlight(element: Element): void {
    const previous = this.#abcHighlightTimers.get(element);
    if (previous) clearTimeout(previous);
    element.classList.remove("fade-highlight-cursor-abc");
    void element.getBoundingClientRect();
    element.classList.add("fade-highlight-cursor-abc");
    const timer = setTimeout(() => {
      element.classList.remove("fade-highlight-cursor-abc");
      this.#abcHighlightTimers.delete(element);
    }, 1_000);
    this.#abcHighlightTimers.set(element, timer);
  }
}

function sourceOffset(
  source: string,
  line: number,
  character: number,
): number | undefined {
  if (!Number.isSafeInteger(line) || line < 0 || character < 0) return;
  const starts = [0];
  for (const match of source.matchAll(/\r\n|\r|\n/g))
    starts.push((match.index ?? 0) + match[0].length);
  const start = starts[line];
  if (start === undefined) return;
  const end = source.slice(start).search(/\r|\n/);
  const lineLength = end < 0 ? source.length - start : end;
  return start + Math.min(character, lineLength);
}
