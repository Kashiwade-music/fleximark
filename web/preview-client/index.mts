import {
  createAssetUrls,
  resolveAssetUrls,
  revokeAssetUrls,
  validAssets,
} from "./assets.mjs";
import {
  parseContent,
  validNavigation,
  validStyle,
} from "./content-security.mjs";
import type { NavigationEntry, RenderFrame } from "./protocol.mjs";
import { isRenderFrame } from "./protocol.mjs";

export type {
  NavigationEntry,
  RenderAsset,
  RenderBlock,
  RenderFrame,
  RenderStyle,
  SourcePosition,
  SourceRange,
} from "./protocol.mjs";

interface DisplayedBlock {
  readonly html: string;
  readonly element: HTMLElement;
  readonly assetReferences: readonly string[];
}

interface DisplayedAnnotations {
  readonly json: string;
  readonly element: HTMLScriptElement;
}

/** Applies complete frames while preserving unchanged top-level block DOM. */
export class PreviewDocument {
  readonly #root: HTMLElement;
  readonly #style: HTMLStyleElement;
  readonly #requestFrame: () => void;
  readonly #highlightTimers = new Map<
    HTMLElement,
    ReturnType<typeof setTimeout>
  >();
  #sessionId?: string;
  #revision = 0;
  #rendererFingerprint?: string;
  #navigation: NavigationEntry[] = [];
  #assetUrls = new Map<string, string>();
  #blocks = new Map<string, DisplayedBlock>();
  #annotations?: DisplayedAnnotations;
  #rejectedFrame?: string;

  constructor(root: HTMLElement, requestFrame: () => void) {
    this.#root = root;
    this.#requestFrame = requestFrame;
    this.#style = document.createElement("style");
    this.#style.dataset.fleximarkTheme = "true";
    document.head.append(this.#style);
  }

  get navigation(): readonly NavigationEntry[] {
    return this.#navigation;
  }

  get previewSessionId(): string | undefined {
    return this.#sessionId;
  }

  get renderRevision(): number {
    return this.#revision;
  }

  apply(frame: RenderFrame): boolean {
    if (!isRenderFrame(frame)) return this.#reject();
    if (
      frame.previewSessionId === this.#sessionId &&
      frame.renderRevision <= this.#revision
    )
      return false;
    const frameKey = `${frame.previewSessionId}:${frame.renderRevision}`;
    const reject = (createdUrls?: ReadonlyMap<string, string>) =>
      this.#reject(createdUrls, frameKey);
    if (!validStyle(frame.style) || !validAssets(frame.assets)) return reject();

    const sameRenderSession = frame.previewSessionId === this.#sessionId;
    const blockIds = new Set<string>();
    const nodeIds = new Set<string>();
    const nextBlocks = new Map<string, DisplayedBlock>();
    const nextElements: HTMLElement[] = [];
    const highlighted: HTMLElement[] = [];
    let addedAssetUrls: Map<string, string>;
    try {
      addedAssetUrls = createAssetUrls(
        frame.assets.filter(({ reference }) => !this.#assetUrls.has(reference)),
      );
    } catch {
      return reject();
    }
    const nextAssetUrls = new Map(this.#assetUrls);
    for (const [reference, url] of addedAssetUrls)
      nextAssetUrls.set(reference, url);

    try {
      for (const block of frame.blocks) {
        if (blockIds.has(block.id)) return reject(addedAssetUrls);
        blockIds.add(block.id);
        for (const nodeId of block.nodeIds) {
          if (nodeIds.has(nodeId)) return reject(addedAssetUrls);
          nodeIds.add(nodeId);
        }
        const previous = this.#blocks.get(block.id);
        const changed = previous !== undefined && previous.html !== block.html;
        let element: HTMLElement;
        let assetReferences: readonly string[];
        if (
          sameRenderSession &&
          frame.rendererFingerprint === this.#rendererFingerprint &&
          previous?.html === block.html
        ) {
          if (
            previous.assetReferences.some(
              (reference) => !nextAssetUrls.has(reference),
            )
          )
            return reject(addedAssetUrls);
          element = previous.element;
          assetReferences = previous.assetReferences;
        } else {
          const parsed = parseContent(block.html, block.id, block.nodeIds);
          if (!parsed) return reject(addedAssetUrls);
          assetReferences = referencedAssets(parsed);
          if (
            assetReferences.some(
              (reference) => !nextAssetUrls.has(reference),
            ) ||
            !resolveAssetUrls(parsed, nextAssetUrls)
          )
            return reject(addedAssetUrls);
          element = parsed;
        }
        nextBlocks.set(block.id, {
          html: block.html,
          element,
          assetReferences,
        });
        nextElements.push(element);
        if (changed) highlighted.push(element);
      }

      const identities = new Map<string, HTMLElement>();
      for (const { element } of nextBlocks.values()) {
        const elements = [
          element,
          ...element.querySelectorAll<HTMLElement>("[data-fleximark-node-id]"),
        ];
        for (const candidate of elements) {
          const nodeId = candidate.dataset.fleximarkNodeId;
          if (!nodeId || identities.has(nodeId)) return reject(addedAssetUrls);
          identities.set(nodeId, candidate);
        }
      }
      if (
        identities.size !== nodeIds.size ||
        !validNavigation(frame.navigation, identities)
      )
        return reject(addedAssetUrls);

      const annotationJson = annotationJsonFor(frame.annotations);
      const previousAnnotations = this.#annotations;
      const annotationScript =
        previousAnnotations && annotationJson === previousAnnotations.json
          ? previousAnnotations.element
          : annotationElement(annotationJson);
      this.#clearHighlights();
      reconcileChildren(this.#root, [
        ...(annotationScript ? [annotationScript] : []),
        ...nextElements,
      ]);
      for (const element of highlighted) this.#highlight(element);
      this.#sessionId = frame.previewSessionId;
      this.#revision = frame.renderRevision;
      this.#rendererFingerprint = frame.rendererFingerprint;
      this.#navigation = frame.navigation;
      this.#blocks = nextBlocks;
      this.#annotations =
        annotationJson && annotationScript
          ? { json: annotationJson, element: annotationScript }
          : undefined;
      this.#rejectedFrame = undefined;
      const css = frame.style?.css ?? "";
      if (this.#style.textContent !== css) this.#style.textContent = css;

      const retainedReferences = new Set(
        frame.assets.map(({ reference }) => reference),
      );
      revokeAssetUrls(
        [...this.#assetUrls]
          .filter(([reference]) => !retainedReferences.has(reference))
          .map(([, url]) => url),
      );
      this.#assetUrls = new Map(
        [...nextAssetUrls].filter(([reference]) =>
          retainedReferences.has(reference),
        ),
      );
      return true;
    } catch {
      return reject(addedAssetUrls);
    }
  }

  dispose(): void {
    this.#clearHighlights();
    this.#style.remove();
    revokeAssetUrls(this.#assetUrls.values());
    this.#assetUrls.clear();
    this.#blocks.clear();
    this.#annotations = undefined;
  }

  #highlight(element: HTMLElement): void {
    element.classList.add("fade-highlight");
    const timer = setTimeout(() => {
      if (this.#highlightTimers.get(element) !== timer) return;
      element.classList.remove("fade-highlight");
      this.#highlightTimers.delete(element);
    }, 1_000);
    this.#highlightTimers.set(element, timer);
  }

  #clearHighlights(): void {
    for (const [element, timer] of this.#highlightTimers) {
      clearTimeout(timer);
      element.classList.remove("fade-highlight");
    }
    this.#highlightTimers.clear();
  }

  #reject(createdUrls?: ReadonlyMap<string, string>, frameKey?: string): false {
    if (createdUrls) revokeAssetUrls(createdUrls.values());
    if (frameKey && this.#rejectedFrame === frameKey) return false;
    this.#rejectedFrame = frameKey;
    this.#requestFrame();
    return false;
  }
}

function annotationJsonFor(
  annotations: Readonly<Record<string, string>>,
): string | undefined {
  if (Object.keys(annotations).length === 0) return;
  return JSON.stringify(annotations);
}

function annotationElement(
  json: string | undefined,
): HTMLScriptElement | undefined {
  if (!json) return;
  const script = document.createElement("script");
  script.type = "application/json";
  script.dataset.fleximarkRenderAnnotations = "";
  script.textContent = json;
  return script;
}

/** Makes direct children match a complete frame without detaching retained nodes. */
function reconcileChildren(root: HTMLElement, desired: readonly Node[]): void {
  const retained = new Set(desired);
  let current = root.firstChild;

  for (const node of desired) {
    while (
      current !== null &&
      node.parentNode === root &&
      !retained.has(current)
    ) {
      const next = current.nextSibling;
      root.removeChild(current);
      current = next;
    }

    if (current === node) {
      current = current.nextSibling;
    } else if (current !== null && !retained.has(current)) {
      const next = current.nextSibling;
      root.replaceChild(node, current);
      current = next;
    } else {
      root.insertBefore(node, current);
    }
  }

  while (current !== null) {
    const next = current.nextSibling;
    root.removeChild(current);
    current = next;
  }
}

function referencedAssets(root: HTMLElement): string[] {
  const references = new Set<string>();
  for (const element of [root, ...root.querySelectorAll<HTMLElement>("*")]) {
    for (const name of ["src", "href", "xlink:href"]) {
      const value = element.getAttribute(name);
      if (value?.startsWith("fleximark-asset:")) references.add(value);
    }
  }
  return [...references];
}
