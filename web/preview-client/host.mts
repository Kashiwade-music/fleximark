import { PreviewEnhancer } from "./enhance.mjs";
import { PreviewDocument, type RenderPublication } from "./index.mjs";
import {
  type EditorNavigationEvent,
  PreviewNavigation,
  type PreviewNavigationEvent,
} from "./navigation.mjs";
import { previewRuntimes } from "./runtimes.mjs";

export type PreviewHostEvent = RenderPublication | PreviewNavigationEvent;

export type PreviewHostMessage =
  | {
      type: "initializePreview";
      messageToken: string;
      publication: RenderPublication;
    }
  | { type: "previewEvent"; messageToken: string; event: PreviewHostEvent };

function isPosition(value: unknown): boolean {
  if (!value || typeof value !== "object") return false;
  const position = value as Record<string, unknown>;
  return (
    Number.isSafeInteger(position.line) &&
    (position.line as number) >= 0 &&
    Number.isSafeInteger(position.character) &&
    (position.character as number) >= 0
  );
}

export function isPreviewHostMessageEvent(
  value: unknown,
  messageToken: string,
): value is PreviewHostMessage {
  return isPreviewHostMessage(value) && value.messageToken === messageToken;
}

export function isPreviewHostMessage(
  value: unknown,
): value is PreviewHostMessage {
  if (!value || typeof value !== "object") return false;
  const message = value as Record<string, unknown>;
  if (typeof message.messageToken !== "string") return false;
  const payload =
    message.type === "initializePreview"
      ? message.publication
      : message.type === "previewEvent"
        ? message.event
        : undefined;
  if (!payload || typeof payload !== "object") return false;
  const event = payload as Record<string, unknown>;
  if (event.type === "full")
    return (
      typeof event.previewSessionId === "string" &&
      typeof event.resultRenderRevision === "number" &&
      typeof event.html === "string" &&
      Array.isArray(event.nodeIds) &&
      Array.isArray(event.navigation) &&
      Array.isArray(event.assets)
    );
  if (event.type === "patch")
    return (
      typeof event.previewSessionId === "string" &&
      typeof event.baseRenderRevision === "number" &&
      typeof event.resultRenderRevision === "number" &&
      Array.isArray(event.navigation) &&
      Array.isArray(event.operations)
    );
  return (
    typeof event.previewSessionId === "string" &&
    typeof event.renderRevision === "number" &&
    ((event.type === "selection" &&
      Array.isArray(event.nodeIds) &&
      (event.activePosition === undefined ||
        event.activePosition === null ||
        isPosition(event.activePosition))) ||
      (event.type === "viewport" && typeof event.nodeId === "string"))
  );
}

export class PreviewHost {
  readonly #root: HTMLElement;
  readonly #preview: PreviewDocument;
  readonly #enhancer = new PreviewEnhancer(previewRuntimes);
  readonly #navigation: PreviewNavigation;

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
    if (changed) void this.#enhancer.render(this.#root);
  }

  dispose(): void {
    this.#navigation.dispose();
    this.#enhancer.dispose();
    this.#preview.dispose();
  }
}
