import { type PreviewHostEvent } from "@fleximark/contracts/protocol";
import type { PreviewNavigationEvent } from "@fleximark/contracts/protocol";

import { PreviewEnhancer } from "./enhance.mjs";
import { PreviewDocument } from "./index.mjs";
import { PreviewNavigation } from "./navigation.mjs";
import { previewRuntimes } from "./runtimes.mjs";
import { PreviewSession } from "./session.mjs";

export type { PreviewHostEvent } from "@fleximark/contracts/protocol";

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
  readonly #session: PreviewSession;
  #disposed = false;

  constructor(
    root: HTMLElement,
    requestFrame: () => void,
    sendNavigation: (event: PreviewNavigationEvent) => void,
    reportError: (error: unknown) => void = () => undefined,
  ) {
    this.#root = root;
    this.#preview = new PreviewDocument(root, () => undefined);
    this.#session = new PreviewSession({
      apply: (frame) => {
        if (!this.#preview.apply(frame)) return false;
        this.#navigation.setKnownIds(
          this.#preview.navigation.map(({ nodeId }) => nodeId),
          this.#preview.navigation,
        );
        void this.#enhancer.render(this.#root).catch(reportError);
        return true;
      },
      navigate: (event) => this.#navigation.receive(event),
      requestRecovery: requestFrame,
      reportError,
    });
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
    for (const event of events) {
      if ("type" in event) void this.#session.receiveNavigation(event);
      else if (this.#session.receiveFrame(event) === false) return false;
    }
    return true;
  }

  fail(error: unknown): void {
    this.#session.fail(error);
  }

  dispose(): void {
    if (this.#disposed) return;
    this.#disposed = true;
    this.#session.dispose();
    this.#navigation.dispose();
    this.#enhancer.dispose();
    this.#preview.dispose();
  }
}
