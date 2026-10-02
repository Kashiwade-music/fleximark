export interface PreviewRuntimes {
  mermaid: {
    initialize(options: {
      startOnLoad: boolean;
      securityLevel: "strict";
    }): void;
    render(id: string, source: string): Promise<{ svg: string }>;
  };
  abc: {
    render(target: HTMLElement, source: string): unknown[];
    supportsAudio(): boolean;
    createTiming(
      visual: unknown,
      callbacks: {
        beat(
          currentBeat: number,
          totalBeats: number,
          totalTime: number,
          position: { left: number; top: number; height: number },
        ): void;
        event(elements: Element[][] | null): void;
      },
    ): {
      start(position?: number): void;
      pause(): void;
      stop(): void;
      reset(): void;
      setProgress(position: number): void;
      currentMillisecond(): number;
      duration(): number;
    };
    createSynth(): {
      init(options: { visualObj: unknown }): Promise<unknown>;
      prime(): Promise<unknown>;
      start(position?: number): void;
      stop(): unknown;
    };
  };
  math: {
    render(
      source: string,
      target: HTMLElement,
      options: {
        displayMode: boolean;
        output: "htmlAndMathml";
        strict: "warn";
        throwOnError: boolean;
        trust: false;
      },
    ): void;
  };
}
