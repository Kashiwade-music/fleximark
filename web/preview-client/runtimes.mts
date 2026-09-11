import * as abcjs from "abcjs";
import katex from "katex";
import mermaid from "mermaid";

import type { PreviewRuntimes } from "./enhance.mjs";

class OscillatorSynth {
  #visual?: abcjs.TuneObject;
  #context?: AudioContext;
  #oscillators: OscillatorNode[] = [];

  async init({ visualObj }: { visualObj: unknown }): Promise<void> {
    this.#visual = visualObj as abcjs.TuneObject;
  }

  prime(): Promise<void> {
    return Promise.resolve();
  }

  start(): void {
    if (!this.#visual) throw new Error("ABC audio is not initialized");
    this.stop();
    const context = new AudioContext();
    const audio = this.#visual.setUpAudio({});
    const meter = this.#visual.getMeterFraction();
    const meterSize = meter.den ? meter.num / meter.den : 1;
    const timeScale =
      this.#visual.millisecondsPerMeasure(audio.tempo) / 1000 / meterSize;
    const now = context.currentTime + 0.02;
    for (const event of audio.tracks.flat()) {
      if (event.cmd !== "note") continue;
      const start = now + event.start * timeScale;
      const end = start + Math.max(0.02, event.duration * timeScale);
      const oscillator = context.createOscillator();
      const gain = context.createGain();
      oscillator.frequency.value = 440 * 2 ** ((event.pitch - 69) / 12);
      gain.gain.setValueAtTime(Math.max(0.001, event.volume / 1270), start);
      gain.gain.exponentialRampToValueAtTime(0.0001, end);
      oscillator.connect(gain).connect(context.destination);
      oscillator.start(start);
      oscillator.stop(end);
      this.#oscillators.push(oscillator);
    }
    this.#context = context;
  }

  stop(): void {
    for (const oscillator of this.#oscillators) {
      try {
        oscillator.stop();
      } catch {
        // The oscillator already ended.
      }
      oscillator.disconnect();
    }
    this.#oscillators = [];
    if (this.#context) void this.#context.close();
    this.#context = undefined;
  }
}

export const previewRuntimes: PreviewRuntimes = {
  mermaid,
  abc: {
    render: (target, source) => abcjs.renderAbc(target, source),
    supportsAudio: () => typeof AudioContext !== "undefined",
    createSynth: () => new OscillatorSynth(),
  },
  math: katex,
};
