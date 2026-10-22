export function previewPayload(block: HTMLElement): string {
  const script = [...block.children].find(
    (child) =>
      child instanceof HTMLScriptElement && child.type === "application/json",
  );
  const value = JSON.parse(script?.textContent ?? "null");
  if (typeof value !== "string") throw new Error("invalid preview payload");
  return value;
}

export function replaceOutput(block: HTMLElement): HTMLElement {
  block.querySelector(":scope > [data-fleximark-output]")?.remove();
  block.querySelector(":scope > [data-fleximark-audio]")?.remove();
  const output = document.createElement(
    block.tagName === "SPAN" ? "span" : "div",
  );
  output.dataset.fleximarkOutput = "true";
  block.append(output);
  return output;
}
