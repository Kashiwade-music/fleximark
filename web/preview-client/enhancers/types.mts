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
    createSynth(): {
      init(options: { visualObj: unknown }): Promise<unknown>;
      prime(): Promise<unknown>;
      start(): void;
      stop(): unknown;
    };
  };
  math: {
    render(
      source: string,
      target: HTMLElement,
      options: {
        displayMode: boolean;
        output: "mathml";
        strict: "error";
        throwOnError: boolean;
        trust: false;
      },
    ): void;
  };
}
