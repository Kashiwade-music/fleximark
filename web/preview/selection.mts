import type { TextPosition } from "@fleximark/contracts/protocol";

import type { NavigationEntry } from "./index.mjs";

export const PREVIEW_SELECTION_EVENT = "fleximark-preview-selection";

export interface PreviewSelectionDetail {
  readonly activePosition?: TextPosition | null;
  readonly navigation?: NavigationEntry;
}

export function dispatchPreviewSelection(
  element: HTMLElement,
  detail: PreviewSelectionDetail,
): void {
  const event = new Event(PREVIEW_SELECTION_EVENT);
  Object.defineProperty(event, "detail", { value: detail });
  element.dispatchEvent(event);
}
