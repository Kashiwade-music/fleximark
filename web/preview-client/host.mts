import { AsyncTaskObserver } from "./async-tasks.mjs";
import { PreviewEnhancer } from "./enhance.mjs";
import { PreviewDocument, type RenderPublication } from "./index.mjs";
import {
  type EditorNavigationEvent,
  PreviewNavigation,
} from "./navigation.mjs";
import {
  type PreviewHostEvent,
  isPreviewHostEvent,
  isRenderPublication,
} from "./protocol.mjs";
import { previewRuntimes } from "./runtimes.mjs";

export type { PreviewHostEvent } from "./protocol.mjs";

export type PreviewHostMessage =
  | {
      type: "initializePreview";
      messageToken: string;
      publication: RenderPublication;
    }
  | { type: "previewEvent"; messageToken: string; event: PreviewHostEvent };

const object = (value: unknown): value is Record<string, unknown> =>
  value !== null && typeof value === "object" && !Array.isArray(value);

export function isPreviewHostMessageEvent(
  value: unknown,
  messageToken: string,
): value is PreviewHostMessage {
  return isPreviewHostMessage(value) && value.messageToken === messageToken;
}

export function isPreviewHostMessage(
  value: unknown,
): value is PreviewHostMessage {
  if (!object(value) || typeof value.messageToken !== "string") return false;
  if (value.type === "initializePreview")
    return (
      Object.keys(value).every((key) =>
        ["type", "messageToken", "publication"].includes(key),
      ) && isRenderPublication(value.publication)
    );
  return (
    value.type === "previewEvent" &&
    Object.keys(value).every((key) =>
      ["type", "messageToken", "event"].includes(key),
    ) &&
    isPreviewHostEvent(value.event)
  );
}

export function isInvalidAuthenticatedPublicationMessage(
  value: unknown,
  messageToken: string,
): boolean {
  if (
    value === null ||
    typeof value !== "object" ||
    Array.isArray(value) ||
    isPreviewHostMessageEvent(value, messageToken)
  )
    return false;
  const message = value as Record<string, unknown>;
  if (message.messageToken !== messageToken) return false;
  if (message.type === "initializePreview") return true;
  const payload = message.type === "previewEvent" ? message.event : undefined;
  return (
    payload !== null &&
    typeof payload === "object" &&
    !Array.isArray(payload) &&
    ["full", "patch"].includes(
      (payload as Record<string, unknown>).type as string,
    )
  );
}

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
    requestSnapshot: () => void,
    sendNavigation: (event: EditorNavigationEvent) => void,
  ) {
    this.#root = root;
    this.#preview = new PreviewDocument(root, requestSnapshot);
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

  apply(events: readonly PreviewHostEvent[]): void {
    if (this.#disposed) return;
    let changed = false;
    for (const event of events) {
      if (event.type === "selection" || event.type === "viewport") {
        if (
          event.previewSessionId !== this.#preview.previewSessionId ||
          event.renderRevision !== this.#preview.renderRevision
        )
          continue;
        this.#navigation.receive(event);
        continue;
      }
      if (!this.#preview.apply(event)) break;
      this.#navigation.setKnownIds(
        this.#preview.navigation.map(({ nodeId }) => nodeId),
        this.#preview.navigation,
      );
      changed = true;
    }
    if (changed && !this.#disposed)
      this.#tasks.observe(this.#enhancer.render(this.#root));
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
