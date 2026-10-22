import {
  type PreviewNavigationEvent,
  isPreviewChangedParams,
  isPreviewNavigationEvent,
  isRenderFrame,
} from "../../contracts/typescript/protocol.mjs";
import { BrowserNavigationTransport } from "./browser-navigation.mjs";
import { PreviewFailureGuard, PreviewHost } from "./host.mjs";
import type { RenderFrame } from "./index.mjs";

interface PreviewHandle {
  dispose(): void;
}

declare global {
  interface Window {
    FlexiMarkPreview: {
      boot(frames: readonly RenderFrame[]): PreviewHandle;
    };
  }
}

const root = document.querySelector<HTMLElement>("#preview");
if (!root) throw new Error("preview root is missing");

window.FlexiMarkPreview = {
  boot(frames) {
    const preview = new PreviewHost(
      root,
      () => undefined,
      () => undefined,
    );
    for (const frame of frames) {
      if (!isRenderFrame(frame)) break;
      preview.apply([frame]);
    }
    return preview;
  },
};

if (document.currentScript?.hasAttribute("data-fleximark-live")) {
  const events = new EventSource(`${location.pathname}/events`);
  const navigation = new BrowserNavigationTransport(
    `${location.pathname}/navigation`,
  );
  const state: {
    preview?: PreviewHost;
    reading: boolean;
    pendingRevision: number;
    pendingNavigation?: PreviewNavigationEvent;
    navigationSequence: number;
  } = { reading: false, pendingRevision: 0, navigationSequence: 0 };
  const failure = new PreviewFailureGuard(
    () => events.close(),
    () => {
      navigation.dispose();
      state.preview?.dispose();
      state.preview = undefined;
    },
    () => {
      root.replaceChildren("Preview unavailable. Reopen it to retry.");
    },
  );
  state.preview = new PreviewHost(
    root,
    () => void readFrame(),
    (event) => navigation.send(event),
  );

  async function readFrame(): Promise<void> {
    if (failure.failed || state.reading) return;
    state.reading = true;
    try {
      do {
        const requestedRevision = state.pendingRevision;
        const response = await fetch(`${location.pathname}/frame`, {
          headers: { accept: "application/json" },
          cache: "no-store",
        });
        if (!response.ok)
          throw new Error(`frame request failed: ${response.status}`);
        const value = (await response.json()) as unknown;
        if (
          value === null ||
          typeof value !== "object" ||
          Array.isArray(value) ||
          !isRenderFrame((value as { frame?: unknown }).frame)
        )
          throw new Error("invalid preview frame");
        const frame = (value as { frame: RenderFrame }).frame;
        state.preview?.apply([frame]);
        if (state.preview?.renderRevision !== frame.renderRevision)
          throw new Error("preview frame was rejected");
        const retained = (value as { navigation?: unknown }).navigation;
        if (retained !== undefined && retained !== null) {
          if (typeof retained !== "object" || Array.isArray(retained))
            throw new Error("invalid retained navigation");
          const { sequence, event } = retained as {
            sequence?: unknown;
            event?: unknown;
          };
          if (
            !Number.isSafeInteger(sequence) ||
            (sequence as number) < 1 ||
            !isPreviewNavigationEvent(event)
          )
            throw new Error("invalid retained navigation");
          const navigationEvent = event as PreviewNavigationEvent;
          if (
            (sequence as number) > state.navigationSequence &&
            navigationEvent.previewSessionId === frame.previewSessionId &&
            navigationEvent.renderRevision === frame.renderRevision
          ) {
            state.preview?.apply([navigationEvent]);
            state.navigationSequence = sequence as number;
            state.pendingNavigation = undefined;
          }
        }
        const pending = state.pendingNavigation;
        if (
          pending?.previewSessionId === frame.previewSessionId &&
          pending.renderRevision <= frame.renderRevision
        ) {
          if (pending.renderRevision === frame.renderRevision)
            state.preview?.apply([pending]);
          state.pendingNavigation = undefined;
        }
        if (requestedRevision === state.pendingRevision) break;
      } while (!failure.failed);
    } catch {
      failure.fail();
    } finally {
      state.reading = false;
    }
  }

  events.addEventListener("message", (event) => {
    if (failure.failed) return;
    try {
      const changed = JSON.parse(
        (event as MessageEvent<string>).data,
      ) as unknown;
      if (isPreviewChangedParams(changed)) {
        const notification =
          changed as import("../../contracts/typescript/protocol.mjs").PreviewChangedParams;
        state.pendingRevision = Math.max(
          state.pendingRevision,
          notification.renderRevision,
        );
        void readFrame();
      } else if (isPreviewNavigationEvent(changed)) {
        const event = changed as PreviewNavigationEvent;
        if (
          state.preview?.previewSessionId === event.previewSessionId &&
          state.preview.renderRevision === event.renderRevision
        )
          state.preview.apply([event]);
        else if (
          !state.pendingNavigation ||
          event.renderRevision >= state.pendingNavigation.renderRevision
        )
          state.pendingNavigation = event;
      } else {
        throw new Error("invalid preview event");
      }
    } catch {
      failure.fail();
    }
  });
  window.addEventListener("unload", () => {
    if (!failure.failed) {
      events.close();
      navigation.dispose();
      state.preview?.dispose();
    }
  });
  void readFrame();
}
