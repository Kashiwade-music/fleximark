import type { TrackedAudio } from "./audio.mjs";
import { previewPayload, replaceOutput } from "./shared.mjs";
import type { PreviewRuntimes } from "./types.mjs";

interface AbcSelectable {
  absEl: { abcelem: { startChar: number; endChar: number } };
  svgEl: Element;
}

function abcSelectables(visual: unknown): AbcSelectable[] {
  if (!visual || typeof visual !== "object") return [];
  const getSelectableArray = Reflect.get(visual, "getSelectableArray");
  if (typeof getSelectableArray !== "function") return [];
  const values: unknown = Reflect.apply(getSelectableArray, visual, []);
  if (!Array.isArray(values)) return [];
  return values.filter((value): value is AbcSelectable => {
    if (!value || typeof value !== "object") return false;
    const selectable = value as Partial<AbcSelectable>;
    const range = selectable.absEl?.abcelem;
    return (
      selectable.svgEl instanceof Element &&
      Number.isSafeInteger(range?.startChar) &&
      Number.isSafeInteger(range?.endChar) &&
      (range?.startChar ?? -1) >= 0 &&
      (range?.endChar ?? -1) > (range?.startChar ?? -1)
    );
  });
}

export interface AbcEnhancerDependencies {
  current(): boolean;
  track(synth: ReturnType<PreviewRuntimes["abc"]["createSynth"]>): TrackedAudio;
  release(handle: TrackedAudio): void;
  report(error: unknown): void;
}

export function renderAbc(
  block: HTMLElement,
  runtime: PreviewRuntimes["abc"],
  dependencies: AbcEnhancerDependencies,
): void {
  const output = replaceOutput(block);
  const visual = runtime.render(output, previewPayload(block))[0];
  for (const selectable of abcSelectables(visual)) {
    selectable.svgEl.setAttribute(
      "data-relative-char-number-start",
      String(selectable.absEl.abcelem.startChar),
    );
    selectable.svgEl.setAttribute(
      "data-relative-char-number-end",
      String(selectable.absEl.abcelem.endChar),
    );
  }
  if (!visual || !runtime.supportsAudio()) return;
  const button = document.createElement("button");
  button.type = "button";
  button.textContent = "Play";
  button.dataset.fleximarkAudio = "play";
  button.addEventListener("click", async () => {
    if (!dependencies.current()) return;
    button.disabled = true;
    let handle: TrackedAudio | undefined;
    try {
      handle = dependencies.track(runtime.createSynth());
      await handle.synth.init({ visualObj: visual });
      await handle.synth.prime();
      if (!dependencies.current()) {
        dependencies.release(handle);
        return;
      }
      handle.synth.start();
    } catch (error) {
      if (handle) dependencies.release(handle);
      if (dependencies.current()) dependencies.report(error);
    } finally {
      if (dependencies.current()) button.disabled = false;
    }
  });
  block.append(button);
}
