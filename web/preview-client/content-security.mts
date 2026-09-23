import type { NavigationEntry, RenderStyle } from "./protocol.mjs";

export function identityMap(
  root: ParentNode,
): Map<string, HTMLElement> | undefined {
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

export function parseContent(
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
    "FORM",
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

export function validNavigation(
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

export function validStyle(style: RenderStyle | null): boolean {
  return (
    style === null ||
    (typeof style.css === "string" &&
      typeof style.fingerprint === "string" &&
      /^[0-9a-f]{64}$/.test(style.fingerprint))
  );
}

export function sameStyle(
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
