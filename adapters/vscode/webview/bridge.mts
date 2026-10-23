import {
  type EditorNavigationEvent,
  type PreviewHostEvent,
  type RenderFrame,
  isEditorNavigationEvent,
  isPreviewHostEvent,
  isRenderFrame,
} from "../../../contracts/typescript/protocol.mjs";

export type WebviewInboundMessage =
  { type: "ready" } | { type: "requestFrame" } | EditorNavigationEvent;
export function isWebviewInboundMessage(
  value: unknown,
): value is WebviewInboundMessage {
  if (!object(value)) return false;
  if (value.type === "ready" || value.type === "requestFrame")
    return Object.keys(value).length === 1;
  return isEditorNavigationEvent(value);
}
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
