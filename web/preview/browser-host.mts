import {
  isRenderFrame,
  isRenderNavigationEvent,
} from "@fleximark/contracts/protocol";

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
  let events: EventSource;
  const navigation = new BrowserNavigationTransport(
    `${location.pathname}/navigation`,
  );
  const preview = new PreviewHost(
    root,
    () => {
      events.close();
      connect();
    },
    (event) => navigation.send(event),
    () => failure.fail(),
  );
  const failure = new PreviewFailureGuard(
    () => events.close(),
    () => navigation.dispose(),
    () => {
      root.setAttribute(
        "data-fleximark-error",
        "Preview unavailable. Reopen it to retry.",
      );
    },
  );
  const connect = () => {
    events = new EventSource(`${location.pathname}/events`);
    events.addEventListener("message", (event: MessageEvent<string>) => {
      if (failure.failed) return;
      try {
        const changed: unknown = JSON.parse(event.data);
        if (isRenderFrame(changed)) {
          preview.apply([changed]);
        } else if (isRenderNavigationEvent(changed)) {
          preview.apply([changed]);
        } else {
          throw new Error("invalid preview event");
        }
      } catch {
        preview.fail(new Error("invalid preview event"));
      }
    });
  };
  connect();
  window.addEventListener("unload", () => {
    if (!failure.failed) {
      events.close();
      navigation.dispose();
      preview.dispose();
    }
  });
}
