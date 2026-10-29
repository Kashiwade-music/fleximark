import * as abcjs from "abcjs";
import katex from "katex";
import mermaid from "mermaid";

import { OscillatorSynth } from "./audio-synth.mjs";
import type { PreviewRuntimes } from "./enhancers/types.mjs";

export const previewRuntimes: PreviewRuntimes = {
  mermaid,
  abc: {
    render: (target, source) =>
      abcjs.renderAbc(target, source, { responsive: "resize" }),
    supportsAudio: () => typeof AudioContext !== "undefined",
    createTiming: (visual, callbacks) => {
      const timing = new abcjs.TimingCallbacks(visual as abcjs.TuneObject, {
        beatCallback: (currentBeat, totalBeats, totalTime, position) =>
          callbacks.beat(currentBeat, totalBeats, totalTime, position),
        eventCallback: (event) => {
          callbacks.event(event?.elements ?? null);
          return event ? "continue" : undefined;
        },
      });
      const duration = Math.max(
        0,
        ...timing.noteTimings.map((event) => event.milliseconds),
      );
      return {
        start: (position) => timing.start(position),
        pause: () => timing.pause(),
        stop: () => timing.stop(),
        reset: () => timing.reset(),
        setProgress: (position) => timing.setProgress(position),
        currentMillisecond: () => timing.currentMillisecond(),
        duration: () => duration,
      };
    },
    createSynth: () => new OscillatorSynth(),
  },
  math: katex,
};
