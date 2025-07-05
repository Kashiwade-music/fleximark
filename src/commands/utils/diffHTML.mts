import { Window, Element, Node, Text } from "happy-dom";

export interface DiffResult {
  selector: string;
  newHTML: string;
}

export function findFirstDiff(
  beforeHTML: string,
  afterHTML: string
): DiffResult | null {
  const windowBefore = new Window();
  const windowAfter = new Window();

  const docBefore = windowBefore.document;
  const docAfter = windowAfter.document;

  docBefore.body.innerHTML = beforeHTML;
  docAfter.body.innerHTML = afterHTML;

  const beforeRoot = docBefore.body;
  const afterRoot = docAfter.body;

  const result = compareNodes(beforeRoot, afterRoot);
  if (!result) return null;

  const { node } = result;

  return {
    selector: getSelector(node),
    newHTML: node.outerHTML,
  };
}

function compareNodes(nodeA: Node, nodeB: Node): { node: Element } | null {
  if (nodeA.nodeType !== nodeB.nodeType) {
    return nodeB instanceof Element ? { node: nodeB } : null;
  }

  if (nodeA instanceof Text && nodeB instanceof Text) {
    if (nodeA.data !== nodeB.data) {
      return nodeB.parentElement ? { node: nodeB.parentElement } : null;
    }
    return null;
  }

  if (!(nodeA instanceof Element) || !(nodeB instanceof Element)) {
    return null;
  }

  if (nodeA.tagName !== nodeB.tagName) {
    return { node: nodeB };
  }

  const attrA = nodeA.attributes;
  const attrB = nodeB.attributes;

  if (attrA.length !== attrB.length) {
    return { node: nodeB };
  }

  for (const name of nodeA.getAttributeNames()) {
    const valA = nodeA.getAttribute(name);
    const valB = nodeB.getAttribute(name);
    if (valA !== valB) {
      return { node: nodeB };
    }
  }

  const childrenA = Array.from(nodeA.childNodes);
  const childrenB = Array.from(nodeB.childNodes);

  if (childrenA.length !== childrenB.length) {
    return { node: nodeB };
  }

  for (let i = 0; i < childrenA.length; i++) {
    const diff = compareNodes(childrenA[i], childrenB[i]);
    if (diff) return diff;
  }

  return null;
}

function getSelector(el: Element): string {
  if (!el.parentElement || el === el.ownerDocument?.body) {
    return "body";
  }

  const siblings = Array.from(el.parentElement.children);
  const index = siblings.indexOf(el) + 1;

  const parentSelector = getSelector(el.parentElement);
  const tag = el.tagName.toLowerCase();

  return `${parentSelector} > ${tag}:nth-child(${index})`;
}
