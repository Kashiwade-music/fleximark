export interface RenderSnapshot {
  type: "full";
  previewSessionId: string;
  documentVersion: number;
  resultRenderRevision: number;
  rendererFingerprint: string;
  nodeIds: string[];
  navigation: NavigationEntry[];
  style: RenderStyle | null;
  assets: RenderAsset[];
  html: string;
}

export interface RenderStyle {
  css: string;
  fingerprint: string;
}

export interface RenderAsset {
  reference: string;
  mediaType: string;
  contentHash: string;
  byteLength: number;
  data: string;
}

export interface NavigationEntry {
  nodeId: string;
  sourceRange: {
    byteStart: number;
    byteEnd: number;
    start: SourcePosition;
    end: SourcePosition;
  };
  depth: number;
}

export interface SourcePosition {
  line: number;
  character: number;
  encoding: "utf8" | "utf16" | "utf32";
}

interface Precondition {
  nodeExists: true;
  currentParentId: string;
}

export type PatchOperation =
  | {
      type: "insert";
      nodeId: string;
      parentId: string;
      beforeId: string | null;
      afterId: string | null;
      atEnd: boolean;
      contentNodeIds: string[];
      content: string;
    }
  | {
      type: "remove";
      nodeId: string;
      parentId: string;
      precondition: Precondition;
    }
  | {
      type: "replace";
      nodeId: string;
      parentId: string;
      contentNodeIds: string[];
      content: string;
      precondition: Precondition;
    }
  | {
      type: "move";
      nodeId: string;
      parentId: string;
      beforeId: string | null;
      afterId: string | null;
      atEnd: boolean;
      precondition: Precondition;
    }
  | {
      type: "setAttributes";
      nodeId: string;
      parentId: string;
      attributes: Record<string, string | null>;
      precondition: Precondition;
    };

export interface RenderPatch {
  type: "patch";
  previewSessionId: string;
  documentVersion: number;
  baseRenderRevision: number;
  resultRenderRevision: number;
  baseRendererFingerprint: string;
  resultRendererFingerprint: string;
  navigation: NavigationEntry[];
  style: RenderStyle | null;
  operations: PatchOperation[];
}

export type RenderPublication = RenderSnapshot | RenderPatch;

function identityMap(root: ParentNode): Map<string, HTMLElement> | undefined {
  const elements =
    root instanceof HTMLElement && root.hasAttribute("data-fleximark-node-id")
      ? [root]
      : [];
  elements.push(
    ...root.querySelectorAll<HTMLElement>("[data-fleximark-node-id]"),
  );
  const identities = new Map<string, HTMLElement>();
  for (const element of elements) {
    const id = element.dataset.fleximarkNodeId;
    if (!id || identities.has(id)) return;
    identities.set(id, element);
  }
  return identities;
}

function parseContent(
  html: string,
  rootId: string,
  expectedIds: string[],
): HTMLElement | undefined {
  const template = document.createElement("template");
  template.innerHTML = html;
  const significantNodes = [...template.content.childNodes].filter(
    (node) => node.nodeType !== Node.TEXT_NODE || node.textContent?.trim(),
  );
  if (
    significantNodes.length !== 1 ||
    !(significantNodes[0] instanceof HTMLElement)
  )
    return;
  const root = significantNodes[0];
  if (root.dataset.fleximarkNodeId !== rootId) return;
  const identities = identityMap(root);
  if (!identities || new Set(expectedIds).size !== expectedIds.length) return;
  if (
    identities.size !== expectedIds.length ||
    expectedIds.some((id) => !identities.has(id))
  )
    return;
  const forbiddenElements = new Set([
    "BASE",
    "EMBED",
    "IFRAME",
    "LINK",
    "META",
    "OBJECT",
    "SCRIPT",
  ]);
  for (const element of [root, ...root.querySelectorAll<HTMLElement>("*")]) {
    if (element.tagName === "SCRIPT") {
      if (
        element.getAttribute("type") !== "application/json" ||
        element.hasAttribute("src") ||
        [...element.childNodes].some((node) => node.nodeType !== Node.TEXT_NODE)
      )
        return;
      try {
        JSON.parse(element.textContent ?? "");
      } catch {
        return;
      }
    } else if (forbiddenElements.has(element.tagName)) {
      return;
    }
    for (const attribute of [...element.attributes]) {
      const name = attribute.name.toLowerCase();
      if (
        name.startsWith("on") ||
        name === "srcdoc" ||
        name === "style" ||
        ((name === "href" || name === "src" || name === "xlink:href") &&
          /^\s*(?:javascript|vbscript|file):/i.test(attribute.value)) ||
        (name !== "src" && /^\s*data:/i.test(attribute.value)) ||
        (name === "src" &&
          /^\s*data:/i.test(attribute.value) &&
          !/^\s*data:image\/(?:gif|jpeg|png|webp);base64,/i.test(
            attribute.value,
          ))
      )
        return;
    }
  }
  return root;
}

function validNavigation(
  navigation: readonly NavigationEntry[],
  identities: Map<string, HTMLElement>,
): boolean {
  const ids = new Set<string>();
  return navigation.every(({ nodeId, sourceRange, depth }) => {
    const positions = [sourceRange.start, sourceRange.end];
    if (
      ids.has(nodeId) ||
      !identities.has(nodeId) ||
      !Number.isSafeInteger(depth) ||
      depth < 0 ||
      !Number.isSafeInteger(sourceRange.byteStart) ||
      !Number.isSafeInteger(sourceRange.byteEnd) ||
      sourceRange.byteStart < 0 ||
      sourceRange.byteEnd < sourceRange.byteStart ||
      positions.some(
        ({ line, character, encoding }) =>
          !Number.isSafeInteger(line) ||
          line < 0 ||
          !Number.isSafeInteger(character) ||
          character < 0 ||
          !["utf8", "utf16", "utf32"].includes(encoding),
      ) ||
      sourceRange.start.line > sourceRange.end.line ||
      (sourceRange.start.line === sourceRange.end.line &&
        sourceRange.start.character > sourceRange.end.character)
    )
      return false;
    ids.add(nodeId);
    return true;
  });
}

function validStyle(style: RenderStyle | null): boolean {
  return (
    style === null ||
    (typeof style.css === "string" &&
      typeof style.fingerprint === "string" &&
      /^[0-9a-f]{64}$/.test(style.fingerprint))
  );
}

function sameStyle(
  left: RenderStyle | null,
  right: RenderStyle | null | undefined,
): boolean {
  return (
    left === right ||
    (left !== null &&
      right != null &&
      left.css === right.css &&
      left.fingerprint === right.fingerprint)
  );
}

function validAssets(assets: readonly RenderAsset[]): boolean {
  const references = new Set<string>();
  let total = 0;
  try {
    return assets.every((asset) => {
      const bytes = atob(asset.data);
      total += bytes.length;
      return (
        /^fleximark-asset:[0-9a-f]{64}$/.test(asset.reference) &&
        /^[0-9a-f]{64}$/.test(asset.contentHash) &&
        asset.reference === `fleximark-asset:${asset.contentHash}` &&
        /^(?:image\/(?:png|jpeg|gif|webp)|audio\/(?:mpeg|ogg|wav))$/.test(
          asset.mediaType,
        ) &&
        Number.isSafeInteger(asset.byteLength) &&
        asset.byteLength === bytes.length &&
        asset.byteLength <= 1024 * 1024 &&
        total <= 8 * 1024 * 1024 &&
        !references.has(asset.reference) &&
        (references.add(asset.reference), true)
      );
    });
  } catch {
    return false;
  }
}

function createAssetUrls(assets: readonly RenderAsset[]): Map<string, string> {
  const urls = new Map<string, string>();
  for (const asset of assets) {
    const binary = atob(asset.data);
    const bytes = Uint8Array.from(binary, (character) =>
      character.charCodeAt(0),
    );
    urls.set(
      asset.reference,
      URL.createObjectURL(new Blob([bytes], { type: asset.mediaType })),
    );
  }
  return urls;
}

function resolveAssetUrls(
  root: ParentNode,
  urls: ReadonlyMap<string, string>,
): boolean {
  const elements =
    root instanceof HTMLElement && root.matches("[src], [href], [xlink\\:href]")
      ? [root]
      : [];
  elements.push(
    ...root.querySelectorAll<HTMLElement>("[src], [href], [xlink\\:href]"),
  );
  for (const element of elements) {
    for (const name of ["src", "href", "xlink:href"]) {
      const value = element.getAttribute(name);
      if (!value?.startsWith("fleximark-asset:")) continue;
      const url = urls.get(value);
      if (!url) return false;
      element.setAttribute(name, url);
    }
  }
  return true;
}

export class PreviewDocument {
  readonly #root: HTMLElement;
  readonly #style: HTMLStyleElement;
  readonly #requestSnapshot: () => void;
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
    this.#style.remove();
    for (const url of this.#assetUrls.values()) URL.revokeObjectURL(url);
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
      for (const url of assetUrls.values()) URL.revokeObjectURL(url);
      this.#requestSnapshot();
      return false;
    }
    this.#root.replaceChildren(...content.childNodes);
    this.#sessionId = snapshot.previewSessionId;
    this.#revision = snapshot.resultRenderRevision;
    this.#fingerprint = snapshot.rendererFingerprint;
    this.#navigation = snapshot.navigation;
    this.#renderStyle = snapshot.style;
    for (const url of this.#assetUrls.values()) URL.revokeObjectURL(url);
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

    const shadow = this.#root.cloneNode(true) as HTMLElement;
    const initialIdentities = identityMap(shadow);
    if (!initialIdentities) {
      this.#requestSnapshot();
      return false;
    }
    const expectedIds = new Set(initialIdentities.keys());
    try {
      for (const operation of patch.operations) {
        const identities = identityMap(shadow);
        if (!identities) throw new Error("invalid identity set");
        const target = identities.get(operation.nodeId);
        const parent =
          operation.parentId === "document-root"
            ? shadow
            : identities.get(operation.parentId);
        if (!parent) throw new Error("unknown patch parent");
        if (operation.type !== "insert") {
          const actualParent = target?.parentElement;
          const actualParentId =
            actualParent === shadow
              ? "document-root"
              : actualParent?.dataset.fleximarkNodeId;
          if (
            !target ||
            !operation.precondition.nodeExists ||
            actualParentId !== operation.precondition.currentParentId ||
            (operation.type !== "move" && actualParentId !== operation.parentId)
          ) {
            throw new Error("patch precondition failed");
          }
        } else if (target) {
          throw new Error("duplicate inserted NodeId");
        }
        const existingTarget = target as HTMLElement;

        if (operation.type === "remove") {
          for (const id of identityMap(existingTarget)?.keys() ?? [])
            expectedIds.delete(id);
          existingTarget.remove();
          continue;
        }
        if (operation.type === "setAttributes") {
          const allowed = new Set([
            "role",
            "aria-checked",
            "open",
            "class",
            "data-line-numbers",
            "data-admonition-kind",
          ]);
          for (const [name, value] of Object.entries(operation.attributes)) {
            if (
              !allowed.has(name) ||
              (value !== null &&
                (typeof value !== "string" || value.length > 4096))
            )
              throw new Error("unsafe patch attribute");
            if (value === null) existingTarget.removeAttribute(name);
            else existingTarget.setAttribute(name, value);
          }
          continue;
        }
        const content =
          operation.type === "move"
            ? existingTarget
            : parseContent(
                operation.content,
                operation.nodeId,
                operation.contentNodeIds,
              );
        if (!content) throw new Error("invalid patch content");
        if (!resolveAssetUrls(content, this.#assetUrls))
          throw new Error("unknown render asset");
        if (operation.type !== "move") {
          const contentIds = identityMap(content);
          const replacedIds =
            operation.type === "replace"
              ? new Set(identityMap(existingTarget)?.keys())
              : new Set<string>();
          if (
            !contentIds ||
            [...contentIds].some(
              ([id]) => identities.has(id) && !replacedIds.has(id),
            )
          ) {
            throw new Error("patch content reuses an identity");
          }
          if (operation.type === "replace")
            for (const id of replacedIds) expectedIds.delete(id);
          for (const id of contentIds.keys()) expectedIds.add(id);
        }
        if (operation.type === "replace") {
          existingTarget.replaceWith(content);
          continue;
        }

        const anchorCount =
          Number(operation.beforeId !== null) +
          Number(operation.afterId !== null) +
          Number(operation.atEnd);
        if (anchorCount !== 1)
          throw new Error("patch requires exactly one anchor");
        const before = operation.beforeId
          ? identities.get(operation.beforeId)
          : undefined;
        const after = operation.afterId
          ? identities.get(operation.afterId)
          : undefined;
        if (
          operation.beforeId === operation.nodeId ||
          operation.afterId === operation.nodeId ||
          (operation.beforeId && before?.parentElement !== parent) ||
          (operation.afterId && after?.parentElement !== parent)
        ) {
          throw new Error("unknown insertion anchor");
        }
        parent.insertBefore(content, before ?? after?.nextSibling ?? null);
      }
      const finalIdentities = identityMap(shadow);
      if (
        !finalIdentities ||
        finalIdentities.size !== expectedIds.size ||
        [...expectedIds].some((id) => !finalIdentities.has(id))
      )
        throw new Error("unexpected NodeId set after patch");
      if (!validNavigation(patch.navigation, finalIdentities))
        throw new Error("invalid navigation map");
    } catch {
      this.#requestSnapshot();
      return false;
    }

    this.#root.replaceChildren(...shadow.childNodes);
    this.#revision = patch.resultRenderRevision;
    this.#fingerprint = patch.resultRendererFingerprint;
    this.#navigation = patch.navigation;
    return true;
  }
}
