import { highlightCode } from "./enhancers/highlight.mjs";
import { createEnhancerRegistry } from "./enhancers/registry.mjs";
import type {
  Disposable,
  Enhancer,
  PreviewRuntimes,
} from "./enhancers/types.mjs";

export type { PreviewRuntimes } from "./enhancers/types.mjs";

export class PreviewEnhancer {
  readonly #registry: ReadonlyMap<string, Enhancer>;
  readonly #resources = new WeakMap<HTMLElement, Disposable>();
  readonly #owners = new Set<HTMLElement>();
  #renderId = 0;
  #disposed = false;

  constructor(private readonly runtimes: PreviewRuntimes) {
    this.#registry = createEnhancerRegistry();
    runtimes.mermaid.initialize({
      startOnLoad: false,
      securityLevel: "strict",
    });
  }

  async render(root: HTMLElement): Promise<void> {
    if (this.#disposed) return;
    for (const element of this.#owners) {
      if (!element.isConnected) this.#release(element);
    }
    const elements = root.hasAttribute("data-fleximark-kind") ? [root] : [];
    elements.push(
      ...root.querySelectorAll<HTMLElement>("[data-fleximark-kind]"),
    );
    await Promise.all(
      elements.map(async (element) => {
        const enhancer = this.#registry.get(
          element.dataset.fleximarkKind ?? "",
        );
        if (!enhancer || this.#resources.has(element)) return;
        let active = true;
        const current = () => active && !this.#disposed && element.isConnected;
        // Claim retained DOM before asynchronous enhancement starts.
        this.#owners.add(element);
        const claim = {
          dispose() {
            active = false;
          },
        };
        this.#resources.set(element, claim);
        try {
          const resources = await enhancer.enhance(element, {
            runtimes: this.runtimes,
            current,
            nextId: () => ++this.#renderId,
            report: (error) => {
              if (current()) this.#setError(element, error);
            },
          });
          if (!current()) {
            resources.dispose();
            if (this.#resources.get(element) === claim) this.#release(element);
            return;
          }
          this.#resources.set(element, {
            dispose() {
              active = false;
              resources.dispose();
            },
          });
          delete element.dataset.fleximarkRenderError;
        } catch (error) {
          if (current()) this.#setError(element, error);
          if (this.#resources.get(element) === claim) this.#release(element);
        }
      }),
    );
    if (!this.#disposed && root.isConnected) highlightCode(root);
  }

  dispose(): void {
    if (this.#disposed) return;
    this.#disposed = true;
    for (const element of this.#owners) this.#release(element);
  }

  #release(element: HTMLElement): void {
    this.#resources.get(element)?.dispose();
    this.#resources.delete(element);
    this.#owners.delete(element);
  }

  #setError(element: HTMLElement, error: unknown): void {
    element.dataset.fleximarkRenderError =
      error instanceof Error ? error.message : String(error);
  }
}
