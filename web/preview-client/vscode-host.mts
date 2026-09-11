import { PreviewHost, isPreviewHostMessage } from "./host.mjs";
import type { EditorNavigationEvent } from "./navigation.mjs";

declare const acquireVsCodeApi: () => {
  postMessage(
    message: { type: "ready" | "requestSnapshot" } | EditorNavigationEvent,
  ): void;
};

const vscode = acquireVsCodeApi();
const root = document.querySelector<HTMLElement>("#preview");
if (!root) throw new Error("preview root is missing");
const preview = new PreviewHost(
  root,
  () => vscode.postMessage({ type: "requestSnapshot" }),
  (event) => vscode.postMessage(event),
);

window.addEventListener("message", (event: MessageEvent<unknown>) => {
  if (event.source !== window || !isPreviewHostMessage(event.data)) return;
  const value =
    event.data.type === "initializePreview"
      ? event.data.publication
      : event.data.event;
  preview.apply([value]);
});

window.addEventListener("unload", () => preview.dispose());

vscode.postMessage({ type: "ready" });
