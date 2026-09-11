import { PreviewHost, type PreviewHostEvent } from "./host.mjs";
import type { RenderPublication } from "./index.mjs";

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
    preview.apply(publications);
    return preview;
  },
};

if (document.currentScript?.hasAttribute("data-fleximark-live")) {
  const events = new EventSource(`${location.pathname}/events`);
  const preview = new PreviewHost(
    root,
    () => location.reload(),
    (event) => {
      void fetch(`${location.pathname}/navigation`, {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify(event),
        credentials: "same-origin",
      });
    },
  );
  events.addEventListener("message", (event) => {
    preview.apply(
      JSON.parse((event as MessageEvent<string>).data) as PreviewHostEvent[],
    );
  });
  window.addEventListener("unload", () => {
    events.close();
    preview.dispose();
  });
}
