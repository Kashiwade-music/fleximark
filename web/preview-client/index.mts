import {
  createAssetUrls,
  resolveAssetUrls,
  revokeAssetUrls,
  validAssets,
} from "./assets.mjs";
import {
  identityMap,
  parseContent,
  sameStyle,
  validNavigation,
  validStyle,
} from "./content-security.mjs";
import { preparePatchTransaction } from "./patch-transaction.mjs";
import type {
  NavigationEntry,
  RenderPatch,
  RenderPublication,
  RenderSnapshot,
  RenderStyle,
} from "./protocol.mjs";
import { isRenderPublication } from "./protocol.mjs";

export type {
  NavigationEntry,
  PatchOperation,
  RenderAsset,
  RenderPatch,
  RenderPublication,
  RenderSnapshot,
  RenderStyle,
  SourcePosition,
  SourceRange,
} from "./protocol.mjs";

export class PreviewDocument {
  readonly #root: HTMLElement;
  readonly #style: HTMLStyleElement;
  readonly #requestSnapshot: () => void;
  readonly #highlightTimers = new Map<
    HTMLElement,
    ReturnType<typeof setTimeout>
  >();
  #sessionId?: string;
  #revision = 0;
  #fingerprint?: string;
  #navigation: NavigationEntry[] = [];
  #renderStyle?: RenderStyle | null;
  #assetUrls = new Map<string, string>();

  constructor(root: HTMLElement, requestSnapshot: () => void) {
    this.#root = root;
    this.#requestSnapshot = requestSnapshot;
    this.#style = document.createElement("style");
    this.#style.dataset.fleximarkTheme = "true";
    document.head.append(this.#style);
  }

  apply(publication: RenderPublication): boolean {
    if (!isRenderPublication(publication)) {
      this.#requestSnapshot();
      return false;
    }
    return publication.type === "full"
      ? this.applySnapshot(publication)
      : this.applyPatch(publication);
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

  dispose(): void {
    this.#clearHighlights();
    this.#style.remove();
    revokeAssetUrls(this.#assetUrls.values());
    this.#assetUrls.clear();
  }

  applySnapshot(snapshot: RenderSnapshot): boolean {
    if (
      snapshot.previewSessionId === this.#sessionId &&
      snapshot.resultRenderRevision <= this.#revision
    )
      return false;
    const content = parseContent(
      snapshot.html,
      "document-root",
      snapshot.nodeIds,
    );
    if (!content || !validAssets(snapshot.assets)) {
      this.#requestSnapshot();
      return false;
    }
    const identities = identityMap(content);
    const assetUrls = createAssetUrls(snapshot.assets);
    if (
      !identities ||
      !validNavigation(snapshot.navigation, identities) ||
      !validStyle(snapshot.style) ||
      !resolveAssetUrls(content, assetUrls)
    ) {
      revokeAssetUrls(assetUrls.values());
      this.#requestSnapshot();
      return false;
    }
    this.#clearHighlights();
    this.#root.replaceChildren(...content.childNodes);
    this.#sessionId = snapshot.previewSessionId;
    this.#revision = snapshot.resultRenderRevision;
    this.#fingerprint = snapshot.rendererFingerprint;
    this.#navigation = snapshot.navigation;
    this.#renderStyle = snapshot.style;
    revokeAssetUrls(this.#assetUrls.values());
    this.#assetUrls = assetUrls;
    this.#style.textContent = snapshot.style?.css ?? "";
    return true;
  }

  applyPatch(patch: RenderPatch): boolean {
    if (
      patch.previewSessionId === this.#sessionId &&
      patch.resultRenderRevision <= this.#revision
    )
      return false;
    if (
      patch.previewSessionId !== this.#sessionId ||
      patch.baseRenderRevision !== this.#revision ||
      patch.resultRenderRevision <= patch.baseRenderRevision ||
      patch.baseRendererFingerprint !== this.#fingerprint ||
      patch.baseRendererFingerprint !== patch.resultRendererFingerprint ||
      !validStyle(patch.style) ||
      !sameStyle(patch.style, this.#renderStyle)
    ) {
      this.#requestSnapshot();
      return false;
    }

    const transaction = preparePatchTransaction(
      this.#root,
      patch,
      this.#assetUrls,
    );
    if (!transaction) {
      this.#requestSnapshot();
      return false;
    }

    this.#clearHighlights();
    this.#root.replaceChildren(...transaction.shadow.childNodes);
    const identities = identityMap(this.#root);
    for (const nodeId of transaction.highlightedIds) {
      const element = identities?.get(nodeId);
      if (element) this.#highlight(element);
    }
    this.#revision = patch.resultRenderRevision;
    this.#fingerprint = patch.resultRendererFingerprint;
    this.#navigation = patch.navigation;
    return true;
  }

  #highlight(element: HTMLElement): void {
    element.classList.add("fade-highlight");
    const timer = setTimeout(() => {
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
}
