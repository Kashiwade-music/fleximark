import type { PreviewRuntimes } from "./types.mjs";

export type AudioSynth = ReturnType<PreviewRuntimes["abc"]["createSynth"]>;

export interface TrackedAudio {
  readonly synth: AudioSynth;
  stop(released: () => void): void;
}

export function trackAudio(synth: AudioSynth): TrackedAudio {
  let state: "active" | "pending" | "released" = "active";
  return {
    synth,
    stop(released) {
      if (state === "released") {
        released();
        return;
      }
      if (state === "pending") return;
      let result: unknown;
      try {
        result = synth.stop();
      } catch {
        return;
      }
      if (!isThenable(result)) {
        state = "released";
        released();
        return;
      }
      state = "pending";
      void Promise.resolve(result).then(
        () => {
          state = "released";
          released();
        },
        () => {
          state = "active";
        },
      );
    },
  };
}

export function stopAudio(handles: Set<TrackedAudio>): void {
  for (const handle of handles) {
    handle.stop(() => handles.delete(handle));
  }
}

function isThenable(value: unknown): value is PromiseLike<unknown> {
  if (
    (typeof value !== "object" || value === null) &&
    typeof value !== "function"
  )
    return false;
  try {
    return typeof Reflect.get(value, "then") === "function";
  } catch {
    return true;
  }
}
