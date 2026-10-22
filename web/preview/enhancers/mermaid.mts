import { previewPayload, replaceOutput } from "./shared.mjs";
import type { PreviewRuntimes } from "./types.mjs";

export async function renderMermaid(
  block: HTMLElement,
  runtime: PreviewRuntimes["mermaid"],
  renderId: string,
  current: () => boolean,
): Promise<void> {
  const source = previewPayload(block);
  const output = replaceOutput(block);
  const { svg } = await runtime.render(renderId, source);
  if (current() && output.isConnected) output.innerHTML = svg;
}
