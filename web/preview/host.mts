import {
  type PreviewHostEvent,
  type PreviewNavigationEvent,
  isPreviewNavigationEvent,
  isRenderFrame,
} from "../../contracts/typescript/protocol.mjs";
import { AsyncTaskObserver } from "./async-tasks.mjs";
import { PreviewEnhancer } from "./enhance.mjs";
import { PreviewDocument, type RenderFrame } from "./index.mjs";
import {
  type EditorNavigationEvent,
  PreviewNavigation,
} from "./navigation.mjs";
import { previewRuntimes } from "./runtimes.mjs";

export type { PreviewHostEvent } from "../../contracts/typescript/protocol.mjs";

export class PreviewFailureGuard {
  #failed = false;

  constructor(
    private readonly close: () => void,
    private readonly dispose: () => void,
    private readonly reload: () => void,
  ) {}

  get failed(): boolean {
    return this.#failed;
  }

  fail(): void {
    if (this.#failed) return;
    this.#failed = true;
    this.close();
    this.dispose();
    this.reload();
  }
}

export class PreviewHost {
  readonly #root: HTMLElement;
  readonly #preview: PreviewDocument;
  readonly #enhancer = new PreviewEnhancer(previewRuntimes);
  readonly #navigation: PreviewNavigation;
  readonly #tasks = new AsyncTaskObserver();
  #disposed = false;

  constructor(
    root: HTMLElement,
    requestFrame: () => void,
    sendNavigation: (event: EditorNavigationEvent) => void,
  ) {
    this.#root = root;
    this.#preview = new PreviewDocument(root, requestFrame);
    this.#navigation = new PreviewNavigation(root, (event) => {
      const previewSessionId = this.#preview.previewSessionId;
      if (!previewSessionId || this.#preview.renderRevision < 1) return;
      sendNavigation({
        ...event,
        previewSessionId,
        renderRevision: this.#preview.renderRevision,
      });
    });
  }

  get previewSessionId(): string | undefined {
    return this.#preview.previewSessionId;
  }

  get renderRevision(): number {
    return this.#preview.renderRevision;
  }

  apply(events: readonly PreviewHostEvent[]): boolean {
    if (this.#disposed) return false;
    let changed = false;
    let accepted = true;
    for (const event of events) {
      if (isPreviewNavigationEvent(event)) {
        const navigation = event as PreviewNavigationEvent;
        if (
          navigation.previewSessionId !== this.#preview.previewSessionId ||
          navigation.renderRevision !== this.#preview.renderRevision
        )
          continue;
        this.#navigation.receive(navigation);
        continue;
      }
      if (!isRenderFrame(event) || !this.#preview.apply(event as RenderFrame)) {
        accepted = false;
        break;
      }
      this.#navigation.setKnownIds(
        this.#preview.navigation.map(({ nodeId }) => nodeId),
        this.#preview.navigation,
      );
      changed = true;
    }
    if (changed && !this.#disposed)
      this.#tasks.observe(this.#enhancer.render(this.#root));
    return accepted;
  }

  dispose(): void {
    if (this.#disposed) {
      this.#enhancer.dispose();
      return;
    }
    this.#disposed = true;
    this.#navigation.dispose();
    this.#enhancer.dispose();
    this.#preview.dispose();
  }
}
