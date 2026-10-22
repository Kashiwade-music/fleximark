import { renderAbc } from "./enhancers/abc.mjs";
import {
  type TrackedAudio,
  stopAudio,
  trackAudio,
} from "./enhancers/audio.mjs";
import { highlightCode } from "./enhancers/highlight.mjs";
import { renderMath } from "./enhancers/math.mjs";
import { renderMermaid } from "./enhancers/mermaid.mjs";
import { renderTabs } from "./enhancers/tabs.mjs";
import type { PreviewRuntimes } from "./enhancers/types.mjs";
import { renderYouTube } from "./enhancers/youtube.mjs";

export type { PreviewRuntimes } from "./enhancers/types.mjs";

export class PreviewEnhancer {
  readonly #runtimes: PreviewRuntimes;
  readonly #audio = new Set<TrackedAudio>();
  #renderId = 0;
  #disposed = false;
  readonly #enhanced = new WeakSet<HTMLElement>();
  readonly #audioOwners = new Map<TrackedAudio, HTMLElement>();
  readonly #activeTabs = new Map<string, string>();
  readonly #mermaidViews = new Map<HTMLElement, () => void>();

  constructor(runtimes: PreviewRuntimes) {
    this.#runtimes = runtimes;
    runtimes.mermaid.initialize({
      startOnLoad: false,
      securityLevel: "strict",
    });
  }

  async render(root: HTMLElement): Promise<void> {
    if (this.#disposed) return;
    for (const [block, dispose] of this.#mermaidViews) {
      if (!block.isConnected) {
        dispose();
        this.#mermaidViews.delete(block);
      }
    }
    for (const [handle, block] of this.#audioOwners) {
      if (!block.isConnected)
        handle.stop(() => {
          this.#audio.delete(handle);
          this.#audioOwners.delete(handle);
        });
    }
    const blocks = root.hasAttribute("data-fleximark-kind") ? [root] : [];
    blocks.push(...root.querySelectorAll<HTMLElement>("[data-fleximark-kind]"));
    await Promise.all(
      blocks.map(async (block) => {
        const current = () => !this.#disposed && block.isConnected;
        try {
          if (this.#enhanced.has(block)) return;
          // Claim before awaiting so concurrent frames share retained DOM work.
          this.#enhanced.add(block);
          switch (block.dataset.fleximarkKind) {
            case "mermaid": {
              this.#mermaidViews.get(block)?.();
              this.#mermaidViews.delete(block);
              const dispose = await renderMermaid(
                block,
                this.#runtimes.mermaid,
                `fleximark-mermaid-${++this.#renderId}`,
                current,
              );
              if (dispose) {
                if (current() && block.isConnected)
                  this.#mermaidViews.set(block, dispose);
                else dispose();
              }
              break;
            }
            case "abc":
              renderAbc(block, this.#runtimes.abc, {
                current,
                track: (synth) => {
                  const handle = trackAudio(synth);
                  this.#audio.add(handle);
                  this.#audioOwners.set(handle, block);
                  return handle;
                },
                release: (handle) => {
                  handle.stop(() => {
                    this.#audio.delete(handle);
                    this.#audioOwners.delete(handle);
                  });
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
        } catch (error) {
          this.#enhanced.delete(block);
          if (current()) this.#setError(block, error);
        }
      }),
    );
    if (!this.#disposed && root.isConnected) highlightCode(root);
  }

  dispose(): void {
    if (!this.#disposed) {
      this.#disposed = true;
      this.#activeTabs.clear();
      this.#audioOwners.clear();
      for (const dispose of this.#mermaidViews.values()) dispose();
      this.#mermaidViews.clear();
    }
    stopAudio(this.#audio);
  }

  #setError(block: HTMLElement, error: unknown): void {
    block.dataset.fleximarkRenderError =
      error instanceof Error ? error.message : String(error);
  }
}
