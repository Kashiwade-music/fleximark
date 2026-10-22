import type { EditorNavigationEvent } from "./navigation.mjs";

export class BrowserNavigationTransport {
  readonly #controller = new AbortController();
  #disposed = false;

  constructor(
    private readonly endpoint: string,
    private readonly fetcher: typeof fetch = fetch,
  ) {}

  send(event: EditorNavigationEvent): void {
    if (this.#disposed) return;
    let request: Promise<Response>;
    try {
      request = this.fetcher(this.endpoint, {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify(event),
        credentials: "same-origin",
        signal: this.#controller.signal,
      });
    } catch {
      return;
    }
    void request.then(
      (response) => {
        void response.ok;
      },
      () => undefined,
    );
  }

  dispose(): void {
    if (this.#disposed) return;
    this.#disposed = true;
    this.#controller.abort();
  }
}
