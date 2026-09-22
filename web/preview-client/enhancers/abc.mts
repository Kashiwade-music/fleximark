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

function formatPlaybackTime(seconds: number): string {
  const wholeSeconds = Math.max(0, Math.floor(seconds));
  const minutes = Math.floor(wholeSeconds / 60);
  const remainder = wholeSeconds % 60;
  return `${minutes}:${String(remainder).padStart(2, "0")}`;
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
  const timing = runtime.createTiming(visual, {
    beat: (_currentBeat, _totalBeats, _totalTime, position) => {
      if (!playing || !Number.isFinite(position.left)) return;
      const x = position.left - 2;
      cursor.setAttribute("x1", String(x));
      cursor.setAttribute("x2", String(x));
      cursor.setAttribute("y1", String(position.top));
      cursor.setAttribute("y2", String(position.top + position.height));
    },
    event: (elements) => {
      if (elements === null) {
        if (playing) finish();
        return;
      }
      for (const group of highlighted)
        for (const element of group) element.classList.remove("color");
      highlighted = elements;
      for (const group of highlighted)
        for (const element of group) element.classList.add("color");
    },
  });
  const duration = timing.duration() / 1000;

  const controls = document.createElement("div");
  controls.className = "fleximark-audio-player";
  controls.dataset.fleximarkAudio = "player";
  controls.setAttribute("role", "group");
  controls.setAttribute("aria-label", "ABC playback");
  const button = document.createElement("button");
  button.type = "button";
  button.className = "fleximark-audio-toggle";
  button.dataset.fleximarkAudioState = "play";
  button.setAttribute("aria-label", "Play");
  button.title = "Play";
  button.setAttribute("aria-pressed", "false");

  const progress = document.createElement("input");
  progress.className = "fleximark-audio-progress";
  progress.type = "range";
  progress.min = "0";
  progress.max = String(duration);
  progress.step = "0.01";
  progress.value = "0";
  progress.setAttribute("aria-label", "Playback position");
  progress.disabled = duration <= 0;

  const time = document.createElement("span");
  time.className = "fleximark-audio-time";
  time.setAttribute("aria-live", "off");
  time.textContent = `${formatPlaybackTime(0)} / ${formatPlaybackTime(duration)}`;

  let handle: TrackedAudio | undefined;
  let playing = false;
  let resumeAfterSeek = false;
  let animationFrame: number | undefined;
  let operation = 0;

  const setProgress = (seconds: number) => {
    const bounded = Math.min(duration, Math.max(0, seconds));
    progress.value = String(bounded);
    progress.style.setProperty(
      "--fleximark-audio-progress",
      duration > 0 ? `${(bounded / duration) * 100}%` : "0%",
    );
    time.textContent = `${formatPlaybackTime(bounded)} / ${formatPlaybackTime(duration)}`;
  };
  const setPlaying = (value: boolean) => {
    playing = value;
    button.dataset.fleximarkAudioState = value ? "pause" : "play";
    button.setAttribute("aria-label", value ? "Pause" : "Play");
    button.title = value ? "Pause" : "Play";
    button.setAttribute("aria-pressed", String(value));
  };
  const releaseAudio = () => {
    const currentHandle = handle;
    handle = undefined;
    if (currentHandle) dependencies.release(currentHandle);
  };
  const stopAnimation = () => {
    if (animationFrame === undefined) return;
    cancelAnimationFrame(animationFrame);
    animationFrame = undefined;
  };
  const updateProgress = () => {
    if (!playing) return;
    if (!dependencies.current()) {
      timing.stop();
      releaseAudio();
      setPlaying(false);
      return;
    }
    setProgress(timing.currentMillisecond() / 1000);
    animationFrame = requestAnimationFrame(updateProgress);
  };
  const finish = () => {
    operation++;
    timing.stop();
    stopAnimation();
    releaseAudio();
    clearPlaybackDisplay();
    setPlaying(false);
    setProgress(0);
  };
  const pause = () => {
    operation++;
    if (playing) {
      timing.pause();
      setProgress(timing.currentMillisecond() / 1000);
    }
    stopAnimation();
    releaseAudio();
    setPlaying(false);
  };
  const play = async () => {
    if (!dependencies.current() || duration <= 0) return;
    const currentOperation = ++operation;
    button.disabled = true;
    let nextHandle: TrackedAudio | undefined;
    try {
      nextHandle = dependencies.track(runtime.createSynth());
      await nextHandle.synth.init({ visualObj: visual });
      await nextHandle.synth.prime();
      if (!dependencies.current() || currentOperation !== operation) {
        dependencies.release(nextHandle);
        return;
      }
      const position = Number(progress.value);
      handle = nextHandle;
      setPlaying(true);
      handle.synth.start(position);
      timing.start(position / duration);
      updateProgress();
    } catch (error) {
      timing.stop();
      stopAnimation();
      if (handle === nextHandle) handle = undefined;
      clearPlaybackDisplay();
      setPlaying(false);
      if (nextHandle) dependencies.release(nextHandle);
      if (dependencies.current()) dependencies.report(error);
    } finally {
      if (dependencies.current()) button.disabled = false;
    }
  };
  button.addEventListener("click", () => {
    if (playing) pause();
    else void play();
  });
  progress.addEventListener("input", () => {
    resumeAfterSeek ||= playing;
    if (playing) pause();
    const position = Number(progress.value);
    timing.setProgress(duration > 0 ? position / duration : 0);
    setProgress(position);
  });
  progress.addEventListener("change", () => {
    if (!resumeAfterSeek) return;
    resumeAfterSeek = false;
    void play();
  });
  controls.append(button, progress, time);
  output.insertAdjacentElement("afterend", controls);
}
