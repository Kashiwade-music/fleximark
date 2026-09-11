import {
  PreviewHost,
  isInvalidAuthenticatedPublicationMessage,
  isPreviewHostMessageEvent,
} from "./host.mjs";
import type { EditorNavigationEvent } from "./navigation.mjs";

declare const acquireVsCodeApi: () => {
  postMessage(
    message:
      | { type: "ready" | "requestSnapshot" }
      | {
          type: "rendered";
          previewSessionId: string;
          renderRevision: number;
        }
      | EditorNavigationEvent,
  ): void;
};

const vscode = acquireVsCodeApi();
const root = document.querySelector<HTMLElement>("#preview");
if (!root) throw new Error("preview root is missing");
const messageToken = document.querySelector<HTMLMetaElement>(
  'meta[name="fleximark-message-token"]',
)?.content;
if (!messageToken) throw new Error("preview message token is missing");
const preview = new PreviewHost(
  root,
  () => vscode.postMessage({ type: "requestSnapshot" }),
  (event) => vscode.postMessage(event),
);

window.addEventListener("message", (event: MessageEvent<unknown>) => {
  if (!isPreviewHostMessageEvent(event.data, messageToken)) {
    if (isInvalidAuthenticatedPublicationMessage(event.data, messageToken))
      vscode.postMessage({ type: "requestSnapshot" });
    return;
  }
  const value =
    event.data.type === "initializePreview"
      ? event.data.publication
      : event.data.event;
  preview.apply([value]);
  if (
    (value.type === "full" || value.type === "patch") &&
    preview.previewSessionId === value.previewSessionId &&
    preview.renderRevision === value.resultRenderRevision
  ) {
    vscode.postMessage({
      type: "rendered",
      previewSessionId: preview.previewSessionId,
      renderRevision: preview.renderRevision,
    });
  }
});

window.addEventListener("unload", () => preview.dispose());

vscode.postMessage({ type: "ready" });
