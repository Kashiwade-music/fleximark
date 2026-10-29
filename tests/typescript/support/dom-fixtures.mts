import { parseHTML } from "linkedom";

const previewHtml =
  "<!doctype html><html><body><main id=preview></main></body></html>";

export function previewDom(html = previewHtml) {
  const { window } = parseHTML(html);
  const root = window.document.querySelector<HTMLElement>("#preview");
  if (!root) throw new Error("preview fixture root is missing");
  return { window, root };
}

export function replaceProperties(
  target: object,
  values: PropertyDescriptorMap,
) {
  const originals = Object.keys(values).map(
    (key) => [key, Object.getOwnPropertyDescriptor(target, key)] as const,
  );
  Object.defineProperties(target, values);
  return {
    dispose() {
      for (const [key, descriptor] of originals) {
        if (descriptor) Object.defineProperty(target, key, descriptor);
        else Reflect.deleteProperty(target, key);
      }
    },
  };
}

export function installDomGlobals(
  window: ReturnType<typeof previewDom>["window"],
  extras: PropertyDescriptorMap = {},
) {
  return replaceProperties(globalThis, {
    document: { configurable: true, value: window.document },
    HTMLElement: { configurable: true, value: window.HTMLElement },
    HTMLScriptElement: { configurable: true, value: window.HTMLScriptElement },
    Node: { configurable: true, value: window.Node },
    Element: { configurable: true, value: window.Element },
    Event: { configurable: true, value: window.Event },
    window: { configurable: true, value: window },
    ...extras,
  });
}
