export type PreviewNavigationUpdate =
  | {
      type: "selection";
      nodeIds: string[];
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

  setKnownIds(ids: Iterable<string>): void {
    this.#knownIds = new Set(ids);
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
  }
}
