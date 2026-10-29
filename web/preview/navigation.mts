import type {
  PreviewNavigationEvent,
  RenderNavigationEvent,
} from "@fleximark/contracts/protocol";

import type { NavigationEntry } from "./index.mjs";
import { dispatchPreviewSelection } from "./selection.mjs";

export class PreviewNavigation {
  readonly #root: HTMLElement;
  readonly #send: (
    event: Omit<PreviewNavigationEvent, "previewSessionId" | "renderRevision">,
  ) => void;
  readonly #onClick: (event: Event) => void;
  readonly #onScroll: () => void;
  #knownIds = new Set<string>();
  #navigation = new Map<string, NavigationEntry>();
  #scrollTimer?: ReturnType<typeof setTimeout>;
  #editorScrollY?: number;

  constructor(
    root: HTMLElement,
    send: (
      event: Omit<
        PreviewNavigationEvent,
        "previewSessionId" | "renderRevision"
      >,
    ) => void,
  ) {
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
      if (this.#scrollTimer) clearTimeout(this.#scrollTimer);
      const editorScrollY = this.#editorScrollY;
      this.#editorScrollY = undefined;
      if (editorScrollY !== undefined && window.scrollY === editorScrollY)
        return;
      this.#scrollTimer = setTimeout(() => {
        this.#scrollTimer = undefined;
        const node = this.#visibleNode();
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

  receive(event: RenderNavigationEvent): void {
    if (event.type === "selection") {
      const selected = new Set(event.nodeIds);
      for (const node of this.#root.querySelectorAll<HTMLElement>(
        "[data-fleximark-node-id]",
      )) {
        if (selected.has(node.dataset.fleximarkNodeId ?? "")) {
          node.dataset.fleximarkSelected = "true";
          dispatchPreviewSelection(node, {
            activePosition: event.activePosition,
            navigation: this.#navigation.get(
              node.dataset.fleximarkNodeId ?? "",
            ),
          });
        } else delete node.dataset.fleximarkSelected;
      }
      return;
    }
    if (!this.#knownIds.has(event.nodeId)) return;
    const node = [
      ...this.#root.querySelectorAll<HTMLElement>("[data-fleximark-node-id]"),
    ].find((element) => element.dataset.fleximarkNodeId === event.nodeId);
    if (node) {
      if (this.#scrollTimer) clearTimeout(this.#scrollTimer);
      this.#scrollTimer = undefined;
      this.#editorScrollY = undefined;
      const previousScrollY = window.scrollY;
      node.scrollIntoView({ block: "start", behavior: "instant" });
      if (window.scrollY !== previousScrollY)
        this.#editorScrollY = window.scrollY;
    }
  }

  #visibleNode(): HTMLElement | undefined {
    const candidates = [
      ...this.#root.querySelectorAll<HTMLElement>("[data-fleximark-node-id]"),
    ]
      .filter((node) => this.#knownIds.has(node.dataset.fleximarkNodeId ?? ""))
      .map((node) => ({ node, rect: node.getBoundingClientRect() }))
      .filter(
        ({ rect }) =>
          rect.height > 0 && rect.bottom > 0 && rect.top < window.innerHeight,
      );
    const containers = new Set<HTMLElement>();
    for (const { node } of candidates) {
      let parent = node.parentElement;
      while (parent && parent !== this.#root) {
        containers.add(parent);
        parent = parent.parentElement;
      }
    }
    let closest: { node: HTMLElement; distance: number } | undefined;
    for (const { node, rect } of candidates) {
      if (containers.has(node)) continue;
      const distance = Math.max(0, rect.top);
      if (!closest || distance < closest.distance) closest = { node, distance };
    }
    return closest?.node;
  }

  dispose(): void {
    this.#root.removeEventListener("click", this.#onClick);
    window.removeEventListener("scroll", this.#onScroll);
    if (this.#scrollTimer) clearTimeout(this.#scrollTimer);
  }
}
