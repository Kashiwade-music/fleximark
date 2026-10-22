import { renderAbc } from "./enhancers/abc.mjs";
import {
  type TrackedAudio,
  stopAudio,
  trackAudio,
} from "./enhancers/audio.mjs";
import { highlightCode } from "./enhancers/highlight.mjs";
import { renderMath } from "./enhancers/math.mjs";
import { renderMermaid } from "./enhancers/mermaid.mjs";
import { blockFingerprint } from "./enhancers/shared.mjs";
import { renderTabs } from "./enhancers/tabs.mjs";
import type { PreviewRuntimes } from "./enhancers/types.mjs";
import { renderYouTube } from "./enhancers/youtube.mjs";

export type { PreviewRuntimes } from "./enhancers/types.mjs";

export class PreviewEnhancer {
  readonly #runtimes: PreviewRuntimes;
  readonly #audio = new Set<TrackedAudio>();
  #renderId = 0;
  #generation = 0;
  #disposed = false;
  readonly #blockFingerprints = new Map<string, string>();
  readonly #activeTabs = new Map<string, string>();

  constructor(runtimes: PreviewRuntimes) {
    this.#runtimes = runtimes;
    runtimes.mermaid.initialize({
      startOnLoad: false,
      securityLevel: "strict",
    });
  }

  async render(root: HTMLElement): Promise<void> {
    if (this.#disposed) return;
    const generation = ++this.#generation;
    stopAudio(this.#audio);
    const current = () => generation === this.#generation;
    const blocks = root.hasAttribute("data-fleximark-kind") ? [root] : [];
    blocks.push(...root.querySelectorAll<HTMLElement>("[data-fleximark-kind]"));
    await Promise.all(
      blocks.map(async (block) => {
        const id = block.dataset.fleximarkNodeId;
        try {
          const fingerprint = blockFingerprint(block);
          if (
            id &&
            this.#blockFingerprints.get(id) === fingerprint &&
            block.dataset.fleximarkKind !== "tabs" &&
            block.dataset.fleximarkKind !== "abc" &&
            !(
              block.dataset.fleximarkKind === "youtube" &&
              !block.querySelector("iframe")
            )
          )
            return;
          switch (block.dataset.fleximarkKind) {
            case "mermaid":
              await renderMermaid(
                block,
                this.#runtimes.mermaid,
                `fleximark-mermaid-${++this.#renderId}`,
                current,
              );
              break;
            case "abc":
              renderAbc(block, this.#runtimes.abc, {
                current,
                track: (synth) => {
                  const handle = trackAudio(synth);
                  this.#audio.add(handle);
                  return handle;
                },
                release: (handle) => {
                  handle.stop(() => this.#audio.delete(handle));
                },
                report: (error) => this.#setError(block, error),
              });
              break;
            case "math":
              renderMath(block, this.#runtimes.math);
              break;
            case "youtube":
              renderYouTube(block, current);
              break;
            case "tabs":
              renderTabs(block, this.#activeTabs, () => ++this.#renderId);
              break;
          }
          if (!current() || !block.isConnected) return;
          delete block.dataset.fleximarkRenderError;
          if (id) this.#blockFingerprints.set(id, fingerprint);
        } catch (error) {
          if (current() && block.isConnected) this.#setError(block, error);
        }
      }),
    );
    if (current() && root.isConnected) highlightCode(root);
  }

  dispose(): void {
    if (!this.#disposed) {
      this.#disposed = true;
      this.#generation++;
      this.#blockFingerprints.clear();
      this.#activeTabs.clear();
    }
    stopAudio(this.#audio);
  }

  #setError(block: HTMLElement, error: unknown): void {
    block.dataset.fleximarkRenderError =
      error instanceof Error ? error.message : String(error);
  }
}
