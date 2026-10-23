import {
  type PreviewNavigationEvent,
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
    pendingNavigation?: PreviewNavigationEvent;
  } = {};
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
    () => failure.fail(),
    (event) => navigation.send(event),
  );

  events.addEventListener("message", (event) => {
    if (failure.failed) return;
    try {
      const changed = JSON.parse(
        (event as MessageEvent<string>).data,
      ) as unknown;
      if (isRenderFrame(changed)) {
        const frame = changed as RenderFrame;
        state.preview?.apply([frame]);
        const pending = state.pendingNavigation;
        if (
          pending?.previewSessionId === frame.previewSessionId &&
          pending.renderRevision <= frame.renderRevision
        ) {
          if (pending.renderRevision === frame.renderRevision)
            state.preview?.apply([pending]);
          state.pendingNavigation = undefined;
        }
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
}
