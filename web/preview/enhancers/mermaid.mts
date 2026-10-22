import { previewPayload, replaceOutput } from "./shared.mjs";
import type { PreviewRuntimes } from "./types.mjs";

export async function renderMermaid(
  block: HTMLElement,
  runtime: PreviewRuntimes["mermaid"],
  renderId: string,
  current: () => boolean,
): Promise<(() => void) | undefined> {
  const source = previewPayload(block);
  const output = replaceOutput(block);
  const { svg } = await runtime.render(renderId, source);
  if (!current() || !output.isConnected) return;
  output.innerHTML = svg;
  const diagram = output.querySelector<SVGSVGElement>("svg");
  if (!diagram) return;

  const enlarge = document.createElement("button");
  enlarge.type = "button";
  enlarge.className = "fleximark-mermaid-enlarge";
  enlarge.textContent = "Enlarge diagram";
  enlarge.setAttribute("aria-haspopup", "dialog");
  output.prepend(enlarge);
  let closeView: (() => void) | undefined;
  const openView = (event: Event) => {
    event.stopPropagation();
    if (closeView || !output.isConnected) return;
    const dialog = document.createElement("dialog");
    dialog.className = "fleximark-mermaid-dialog";
    dialog.setAttribute("aria-label", "Enlarged Mermaid diagram");
    const toolbar = document.createElement("div");
    toolbar.className = "fleximark-mermaid-toolbar";
    const viewport = document.createElement("div");
    viewport.className = "fleximark-mermaid-viewport";
    const originalStyle = diagram.getAttribute("style");
    const viewBox = diagram
      .getAttribute("viewBox")
      ?.trim()
      .split(/[\s,]+/);
    const intrinsicWidth = Number(viewBox?.[2]);
    const width =
      intrinsicWidth > 0 && Number.isFinite(intrinsicWidth)
        ? intrinsicWidth
        : diagram.getBoundingClientRect().width || 800;
    let zoom = 1;
    const zoomLabel = document.createElement("output");
    zoomLabel.setAttribute("aria-label", "Diagram zoom");
    const resize = (next: number) => {
      zoom = Math.max(0.25, Math.min(4, next));
      diagram.style.setProperty("width", `${width * zoom}px`, "important");
      diagram.style.setProperty("max-width", "none", "important");
      diagram.style.setProperty("height", "auto", "important");
      zoomLabel.textContent = `${Math.round(zoom * 100)}%`;
    };
    const button = (label: string, action: () => void) => {
      const control = document.createElement("button");
      control.type = "button";
      control.textContent = label;
      control.addEventListener("click", action);
      return control;
    };
    const restore = () => {
      if (!closeView) return;
      closeView = undefined;
      if (originalStyle === null) diagram.removeAttribute("style");
      else diagram.setAttribute("style", originalStyle);
      output.append(diagram);
      dialog.remove();
      if (enlarge.isConnected) enlarge.focus();
    };
    closeView = () => {
      dialog.close();
      restore();
    };
    const close = button("Close", () => closeView?.());
    toolbar.append(
      button("Zoom out", () => resize(zoom / 1.25)),
      zoomLabel,
      button("Zoom in", () => resize(zoom * 1.25)),
      button("Reset zoom", () => resize(1)),
      close,
    );
    dialog.addEventListener("close", restore);
    dialog.addEventListener("cancel", (event) => {
      event.preventDefault();
      closeView?.();
    });
    dialog.addEventListener("click", (event) => {
      if (event.target === dialog) closeView?.();
    });
    viewport.append(diagram);
    dialog.append(toolbar, viewport);
    document.body.append(dialog);
    resize(1);
    dialog.showModal();
    close.focus();
  };
  enlarge.addEventListener("click", openView);
  return () => {
    closeView?.();
    enlarge.removeEventListener("click", openView);
    enlarge.remove();
  };
}
