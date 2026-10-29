import type { PreviewNavigationEvent } from "@fleximark/contracts/protocol";
import { PreviewFailureGuard, PreviewHost } from "@fleximark/preview/host";

import {
  isInvalidAuthenticatedFrameMessage,
  isPreviewHostMessageEvent,
} from "./bridge.mjs";

declare const acquireVsCodeApi: () => {
  postMessage(
    message: { type: "ready" | "requestFrame" } | PreviewNavigationEvent,
  ): void;
};

const vscode = acquireVsCodeApi();
const root = document.querySelector<HTMLElement>("#preview");
if (!root) throw new Error("preview root is missing");
const messageToken = document.querySelector<HTMLMetaElement>(
  'meta[name="fleximark-message-token"]',
)?.content;
if (!messageToken) throw new Error("preview message token is missing");
// Authenticate the first host message by origin and token, then pin its source.
let hostWindow: MessageEventSource | undefined;
const failure = new PreviewFailureGuard(
  () => undefined,
  () => undefined,
  () =>
    root.setAttribute(
      "data-fleximark-error",
      "Preview unavailable. Reopen it to retry.",
    ),
);
const preview = new PreviewHost(
  root,
  () => vscode.postMessage({ type: "requestFrame" }),
  (event) => vscode.postMessage(event),
  () => failure.fail(),
);

window.addEventListener("message", (event: MessageEvent<unknown>) => {
  if (failure.failed) return;
  if (event.source === null || event.origin !== window.location.origin) return;
  if (hostWindow !== undefined && event.source !== hostWindow) return;
  if (!isPreviewHostMessageEvent(event.data, messageToken)) {
    if (isInvalidAuthenticatedFrameMessage(event.data, messageToken)) {
      hostWindow = event.source;
      preview.fail(new Error("invalid authenticated preview frame"));
    }
    return;
  }
  hostWindow = event.source;
  const value =
    event.data.type === "previewFrame" ? event.data.frame : event.data.event;
  preview.apply([value]);
});

window.addEventListener("unload", () => preview.dispose());

vscode.postMessage({ type: "ready" });
