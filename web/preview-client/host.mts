import { AsyncTaskObserver } from "./async-tasks.mjs";
import { PreviewEnhancer } from "./enhance.mjs";
import { PreviewDocument, type RenderFrame } from "./index.mjs";
import {
  type EditorNavigationEvent,
  PreviewNavigation,
} from "./navigation.mjs";
import {
  type PreviewHostEvent,
  type PreviewNavigationEvent,
  isPreviewHostEvent,
  isPreviewNavigationEvent,
  isRenderFrame,
} from "./protocol.mjs";
import { previewRuntimes } from "./runtimes.mjs";

export type { PreviewHostEvent } from "./protocol.mjs";

export type PreviewHostMessage =
  | {
      type: "previewFrame";
      messageToken: string;
      frame: RenderFrame;
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
  if (value.type === "previewFrame")
    return (
      Object.keys(value).every((key) =>
        ["type", "messageToken", "frame"].includes(key),
      ) && isRenderFrame(value.frame)
    );
  return (
    value.type === "previewEvent" &&
    Object.keys(value).every((key) =>
      ["type", "messageToken", "event"].includes(key),
    ) &&
    isPreviewHostEvent(value.event)
  );
}

export function isInvalidAuthenticatedFrameMessage(
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
  return message.type === "previewFrame";
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
