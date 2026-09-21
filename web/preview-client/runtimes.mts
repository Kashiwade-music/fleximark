import * as abcjs from "abcjs";
import katex from "katex";
import mermaid from "mermaid";

import type { PreviewRuntimes } from "./enhancers/types.mjs";

interface ContextResource {
  readonly context: AudioContext;
  closing?: Promise<void>;
}

class OscillatorSynth {
  #visual?: abcjs.TuneObject;
  readonly #contexts = new Set<ContextResource>();
  #oscillators: OscillatorNode[] = [];

  async init({ visualObj }: { visualObj: unknown }): Promise<void> {
    this.#visual = visualObj as abcjs.TuneObject;
  }

  prime(): Promise<void> {
    return Promise.resolve();
  }

  start(): void {
    if (!this.#visual) throw new Error("ABC audio is not initialized");
    void this.stop().catch(() => undefined);
    const context = new AudioContext();
    this.#contexts.add({ context });
    try {
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
        this.#oscillators.push(oscillator);
        const gain = context.createGain();
        oscillator.frequency.value = 440 * 2 ** ((event.pitch - 69) / 12);
        gain.gain.setValueAtTime(Math.max(0.001, event.volume / 1270), start);
        gain.gain.exponentialRampToValueAtTime(0.0001, end);
        oscillator.connect(gain).connect(context.destination);
        oscillator.start(start);
        oscillator.stop(end);
      }
    } catch (error) {
      void this.stop().catch(() => undefined);
      throw error;
    }
  }

  stop(): Promise<void> {
    for (const oscillator of this.#oscillators) {
      try {
        oscillator.stop();
      } catch {
        // The oscillator already ended.
      }
      try {
        oscillator.disconnect();
      } catch {
        // Continue releasing the remaining audio resources.
      }
    }
    this.#oscillators = [];
    const closings = [...this.#contexts].map((resource) =>
      this.#closeContext(resource),
    );
    return Promise.allSettled(closings).then((settlements) => {
      const failure = settlements.find(
        (settlement): settlement is PromiseRejectedResult =>
          settlement.status === "rejected",
      );
      if (failure) throw failure.reason;
    });
  }

  #closeContext(resource: ContextResource): Promise<void> {
    if (resource.closing) return resource.closing;
    let closing: Promise<void>;
    try {
      closing = Promise.resolve(resource.context.close());
    } catch (error) {
      closing = Promise.reject(error);
    }
    resource.closing = closing;
    void closing.then(
      () => this.#contexts.delete(resource),
      () => {
        if (resource.closing === closing) resource.closing = undefined;
      },
    );
    return closing;
  }
}

export const previewRuntimes: PreviewRuntimes = {
  mermaid,
  abc: {
    render: (target, source) =>
      abcjs.renderAbc(target, source, { responsive: "resize" }),
    supportsAudio: () => typeof AudioContext !== "undefined",
    createTiming: (visual, callbacks) => {
      const timing = new abcjs.TimingCallbacks(visual as abcjs.TuneObject, {
        beatCallback: (currentBeat, totalBeats, _totalTime, position) =>
          callbacks.beat(currentBeat, totalBeats, position),
        eventCallback: (event) => {
          callbacks.event(event?.elements ?? null);
          return event ? "continue" : undefined;
        },
      });
      return {
        start: () => timing.start(),
        stop: () => timing.stop(),
        reset: () => timing.reset(),
      };
    },
    createSynth: () => new OscillatorSynth(),
  },
  math: katex,
};
