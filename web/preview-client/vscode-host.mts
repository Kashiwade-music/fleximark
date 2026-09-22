import {
  PreviewFailureGuard,
  PreviewHost,
  isInvalidAuthenticatedFrameMessage,
  isPreviewHostMessageEvent,
} from "./host.mjs";
import type { EditorNavigationEvent } from "./navigation.mjs";

declare const acquireVsCodeApi: () => {
  postMessage(
    message: { type: "ready" | "requestFrame" } | EditorNavigationEvent,
  ): void;
};

const vscode = acquireVsCodeApi();
const root = document.querySelector<HTMLElement>("#preview");
if (!root) throw new Error("preview root is missing");
const messageToken = document.querySelector<HTMLMetaElement>(
  'meta[name="fleximark-message-token"]',
)?.content;
if (!messageToken) throw new Error("preview message token is missing");
let recoveryRequested = false;
const state: { preview?: PreviewHost } = {};
const failure = new PreviewFailureGuard(
  () => undefined,
  () => state.preview?.dispose(),
  () => root.replaceChildren("Preview unavailable. Reopen it to retry."),
);
const requestRecovery = () => {
  if (failure.failed) return;
  if (recoveryRequested) {
    failure.fail();
    return;
  }
  recoveryRequested = true;
  vscode.postMessage({ type: "requestFrame" });
};
const preview = new PreviewHost(root, requestRecovery, (event) =>
  vscode.postMessage(event),
);
state.preview = preview;

window.addEventListener("message", (event: MessageEvent<unknown>) => {
  if (failure.failed) return;
  if (!isPreviewHostMessageEvent(event.data, messageToken)) {
    if (isInvalidAuthenticatedFrameMessage(event.data, messageToken))
      requestRecovery();
    return;
  }
  const value =
    event.data.type === "previewFrame" ? event.data.frame : event.data.event;
  if (event.data.type !== "previewFrame") {
    preview.apply([value]);
    return;
  }
  const stale =
    value.previewSessionId === preview.previewSessionId &&
    value.renderRevision <= preview.renderRevision;
  const recoveryWasRequested = recoveryRequested;
  if (preview.apply([value])) recoveryRequested = false;
  else if (recoveryWasRequested && !stale) failure.fail();
});

window.addEventListener("unload", () => preview.dispose());

vscode.postMessage({ type: "ready" });
