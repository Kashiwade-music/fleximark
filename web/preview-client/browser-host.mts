import { BrowserNavigationTransport } from "./browser-navigation.mjs";
import {
  PreviewFailureGuard,
  PreviewHost,
  type PreviewHostEvent,
} from "./host.mjs";
import type { RenderPublication } from "./index.mjs";
import { isPreviewHostEvent, isRenderPublication } from "./protocol.mjs";

interface PreviewHandle {
  dispose(): void;
}

declare global {
  interface Window {
    FlexiMarkPreview: {
      boot(publications: readonly RenderPublication[]): PreviewHandle;
    };
  }
}

const root = document.querySelector<HTMLElement>("#preview");
if (!root) throw new Error("preview root is missing");

window.FlexiMarkPreview = {
  boot(publications) {
    const preview = new PreviewHost(
      root,
      () => undefined,
      () => undefined,
    );
    if (publications.every(isRenderPublication)) preview.apply(publications);
    return preview;
  },
};

if (document.currentScript?.hasAttribute("data-fleximark-live")) {
  const events = new EventSource(`${location.pathname}/events`);
  const navigation = new BrowserNavigationTransport(
    `${location.pathname}/navigation`,
  );
  const state: { preview?: PreviewHost } = {};
  const failure = new PreviewFailureGuard(
    () => events.close(),
    () => {
      navigation.dispose();
      state.preview?.dispose();
      state.preview = undefined;
    },
    () => location.reload(),
  );
  state.preview = new PreviewHost(
    root,
    () => failure.fail(),
    (event) => navigation.send(event),
  );
  events.addEventListener("message", (event) => {
    if (failure.failed) return;
    try {
      const value = JSON.parse((event as MessageEvent<string>).data) as unknown;
      if (!Array.isArray(value) || !value.every(isPreviewHostEvent)) {
        failure.fail();
        return;
      }
      state.preview?.apply(value as PreviewHostEvent[]);
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
