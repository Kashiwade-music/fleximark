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
  const output = document.createElement("div");
  output.dataset.fleximarkOutput = "true";
  block.append(output);
  return output;
}

export function blockFingerprint(block: HTMLElement): string {
  const kind = block.dataset.fleximarkKind;
  if (kind === "tabs")
    return JSON.stringify(
      [...block.children]
        .filter(
          (child): child is HTMLElement =>
            child instanceof HTMLElement &&
            child.dataset.fleximarkKind === "tab",
        )
        .map((panel) => [
          panel.dataset.fleximarkNodeId,
          panel.dataset.tabLabel,
          panel.textContent,
        ]),
    );
  if (kind === "youtube") return block.dataset.source ?? "";
  if (kind === "mermaid" || kind === "abc") return previewPayload(block);
  if (kind === "math") {
    const payload = [...block.children].find(
      (child) =>
        child instanceof HTMLScriptElement &&
        child.dataset.fleximarkMathSource === "true",
    );
    if (!payload) return block.textContent ?? "";
    const source: unknown = JSON.parse(payload.textContent ?? "null");
    return typeof source === "string" ? source : "";
  }
  return block.textContent ?? "";
}
