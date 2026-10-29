import { isRecord } from "@fleximark/contracts/guards";
import {
  type PreviewHostEvent,
  type PreviewNavigationEvent,
  type RenderFrame,
  isPreviewHostEvent,
  isPreviewNavigationEvent,
  isRenderFrame,
} from "@fleximark/contracts/protocol";

export type WebviewInboundMessage =
  { type: "ready" } | { type: "requestFrame" } | PreviewNavigationEvent;
export function isWebviewInboundMessage(
  value: unknown,
): value is WebviewInboundMessage {
  if (!isRecord(value)) return false;
  if (value.type === "ready" || value.type === "requestFrame")
    return Object.keys(value).length === 1;
  return isPreviewNavigationEvent(value);
}
export type PreviewHostMessage =
  | {
      type: "previewFrame";
      messageToken: string;
      frame: RenderFrame;
    }
  | { type: "previewEvent"; messageToken: string; event: PreviewHostEvent };

export function isPreviewHostMessageEvent(
  value: unknown,
  messageToken: string,
): value is PreviewHostMessage {
  return isPreviewHostMessage(value) && value.messageToken === messageToken;
}

export function isPreviewHostMessage(
  value: unknown,
): value is PreviewHostMessage {
  if (!isRecord(value) || typeof value.messageToken !== "string") return false;
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
  if (!isRecord(value) || isPreviewHostMessageEvent(value, messageToken))
    return false;
  const message = value;
  if (message.messageToken !== messageToken) return false;
  return message.type === "previewFrame";
}
