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
  const svg = output.querySelector("svg");
  const cursor = document.createElementNS("http://www.w3.org/2000/svg", "line");
  cursor.classList.add("abcjs-cursor");
  svg?.append(cursor);
  let highlighted: Element[][] = [];
  const clearPlaybackDisplay = () => {
    for (const group of highlighted)
      for (const element of group) element.classList.remove("color");
    highlighted = [];
    for (const name of ["x1", "x2", "y1", "y2"]) cursor.setAttribute(name, "0");
  };
  const button = document.createElement("button");
  button.type = "button";
  button.textContent = "Play";
  button.dataset.fleximarkAudio = "play";
  button.setAttribute("aria-pressed", "false");
  let active:
    | {
        handle: TrackedAudio;
        timing: ReturnType<PreviewRuntimes["abc"]["createTiming"]>;
      }
    | undefined;
  const finish = () => {
    const playback = active;
    if (!playback) return;
    active = undefined;
    clearPlaybackDisplay();
    button.textContent = "Play";
    button.dataset.fleximarkAudio = "play";
    button.setAttribute("aria-pressed", "false");
    dependencies.release(playback.handle);
  };
  button.addEventListener("click", async () => {
    if (!dependencies.current()) return;
    if (active) {
      const playback = active;
      active = undefined;
      playback.timing.stop();
      clearPlaybackDisplay();
      button.textContent = "Play";
      button.dataset.fleximarkAudio = "play";
      button.setAttribute("aria-pressed", "false");
      dependencies.release(playback.handle);
      return;
    }
    button.disabled = true;
    let handle: TrackedAudio | undefined;
    let timing: ReturnType<PreviewRuntimes["abc"]["createTiming"]> | undefined;
    try {
      handle = dependencies.track(runtime.createSynth());
      timing = runtime.createTiming(visual, {
        beat: (currentBeat, totalBeats, position) => {
          if (!active || currentBeat === totalBeats) {
            if (currentBeat === totalBeats) finish();
            return;
          }
          const x = position.left - 2;
          cursor.setAttribute("x1", String(x));
          cursor.setAttribute("x2", String(x));
          cursor.setAttribute("y1", String(position.top));
          cursor.setAttribute("y2", String(position.top + position.height));
        },
        event: (elements) => {
          if (!active) return;
          if (elements === null) {
            finish();
            return;
          }
          for (const group of highlighted)
            for (const element of group) element.classList.remove("color");
          highlighted = elements;
          for (const group of highlighted)
            for (const element of group) element.classList.add("color");
        },
      });
      await handle.synth.init({ visualObj: visual });
      await handle.synth.prime();
      if (!dependencies.current()) {
        timing.stop();
        dependencies.release(handle);
        return;
      }
      active = { handle, timing };
      button.textContent = "Stop";
      button.dataset.fleximarkAudio = "stop";
      button.setAttribute("aria-pressed", "true");
      handle.synth.start();
      timing.start();
    } catch (error) {
      timing?.stop();
      if (active?.handle === handle) active = undefined;
      clearPlaybackDisplay();
      button.textContent = "Play";
      button.dataset.fleximarkAudio = "play";
      button.setAttribute("aria-pressed", "false");
      if (handle) dependencies.release(handle);
      if (dependencies.current()) dependencies.report(error);
    } finally {
      if (dependencies.current()) button.disabled = false;
    }
  });
  block.append(button);
}
