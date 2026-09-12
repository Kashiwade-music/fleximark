import { resolveAssetUrls } from "./assets.mjs";
import {
  identityMap,
  parseContent,
  validNavigation,
} from "./content-security.mjs";
import { isSafePatchAttribute } from "./patch-attributes.mjs";
import type { RenderPatch } from "./protocol.mjs";

export interface PatchTransaction {
  shadow: HTMLElement;
  highlightedIds: Set<string>;
}

export function preparePatchTransaction(
  root: HTMLElement,
  patch: RenderPatch,
  assetUrls: ReadonlyMap<string, string>,
): PatchTransaction | undefined {
  const shadow = root.cloneNode(true) as HTMLElement;
  for (const element of shadow.querySelectorAll<HTMLElement>(
    ".fade-highlight, .fade-highlight-cursor-abc",
  ))
    element.classList.remove("fade-highlight", "fade-highlight-cursor-abc");
  const initialIdentities = identityMap(shadow);
  if (!initialIdentities) return;
  const expectedIds = new Set(initialIdentities.keys());
  const highlightedIds = new Set<string>();
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
        )
          throw new Error("patch precondition failed");
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
        for (const [name, value] of Object.entries(operation.attributes)) {
          if (!isSafePatchAttribute(name, value))
            throw new Error("unsafe patch attribute");
          if (value === null) existingTarget.removeAttribute(name);
          else existingTarget.setAttribute(name, value);
        }
        highlightedIds.add(operation.nodeId);
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
      if (!resolveAssetUrls(content, assetUrls))
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
        )
          throw new Error("patch content reuses an identity");
        if (operation.type === "replace")
          for (const id of replacedIds) expectedIds.delete(id);
        for (const id of contentIds.keys()) expectedIds.add(id);
      }
      if (operation.type === "replace") {
        existingTarget.replaceWith(content);
        highlightedIds.add(operation.nodeId);
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
      )
        throw new Error("unknown insertion anchor");
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
    return;
  }
  return { shadow, highlightedIds };
}
