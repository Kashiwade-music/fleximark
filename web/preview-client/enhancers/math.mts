import { replaceOutput } from "./shared.mjs";
import type { PreviewRuntimes } from "./types.mjs";

export function renderMath(
  block: HTMLElement,
  runtime: PreviewRuntimes["math"],
): void {
  let source: string;
  const payload = [...block.children].find(
    (child) =>
      child instanceof HTMLScriptElement &&
      child.dataset.fleximarkMathSource === "true",
  );
  if (payload) {
    source = JSON.parse(payload.textContent ?? "");
  } else {
    source = block.textContent ?? "";
    const script = document.createElement("script");
    script.type = "application/json";
    script.dataset.fleximarkMathSource = "true";
    script.textContent = JSON.stringify(source);
    block.replaceChildren(script);
  }
  runtime.render(source, replaceOutput(block), {
    displayMode: block.tagName === "DIV",
    output: "htmlAndMathml",
    strict: "warn",
    throwOnError: false,
    trust: false,
  });
}
