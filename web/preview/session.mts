import type {
  RenderFrame,
  RenderNavigationEvent,
} from "@fleximark/contracts/protocol";

export interface PreviewSessionPorts {
  apply(frame: RenderFrame): boolean | Promise<boolean>;
  navigate(event: RenderNavigationEvent): void | Promise<void>;
  requestRecovery(): void;
  reportError(error: unknown): void;
}

/** Owns frame ordering, deferred navigation and bounded transport recovery. */
export class PreviewSession {
  #sessionId?: string;
  #revision = 0;
  #pending?: RenderNavigationEvent;
  #recoveryRequested = false;
  #disposed = false;
  #tail?: Promise<unknown>;

  constructor(private readonly ports: PreviewSessionPorts) {}

  get previewSessionId(): string | undefined {
    return this.#sessionId;
  }
  get renderRevision(): number {
    return this.#revision;
  }

  receiveFrame(frame: RenderFrame): boolean | Promise<boolean> {
    return this.#enqueue(() => {
      if (
        frame.previewSessionId === this.#sessionId &&
        frame.renderRevision <= this.#revision
      )
        return false;
      const complete = (accepted: boolean): boolean | Promise<boolean> => {
        if (this.#disposed) return false;
        if (!accepted) {
          this.fail(new Error("preview frame rejected"));
          return false;
        }
        this.#sessionId = frame.previewSessionId;
        this.#revision = frame.renderRevision;
        this.#recoveryRequested = false;
        const pending = this.#pending;
        if (!pending) return true;
        if (
          pending.previewSessionId !== this.#sessionId ||
          pending.renderRevision <= this.#revision
        ) {
          this.#pending = undefined;
          if (
            pending.previewSessionId === this.#sessionId &&
            pending.renderRevision === this.#revision
          ) {
            const result = this.ports.navigate(pending);
            if (result instanceof Promise) return result.then(() => true);
          }
        }
        return true;
      };
      const result = this.ports.apply(frame);
      return result instanceof Promise
        ? result.then(complete)
        : complete(result);
    }, false);
  }

  receiveNavigation(event: RenderNavigationEvent): void | Promise<void> {
    return this.#enqueue(() => {
      if (event.previewSessionId === this.#sessionId) {
        if (event.renderRevision < this.#revision) return;
        if (event.renderRevision === this.#revision)
          return this.ports.navigate(event);
      }
      if (
        !this.#pending ||
        event.previewSessionId !== this.#pending.previewSessionId ||
        event.renderRevision >= this.#pending.renderRevision
      )
        this.#pending = event;
    }, undefined);
  }

  fail(error: unknown): void {
    if (this.#disposed) return;
    if (this.#recoveryRequested) {
      this.ports.reportError(error);
      return;
    }
    this.#recoveryRequested = true;
    this.ports.requestRecovery();
  }

  dispose(): void {
    this.#disposed = true;
    this.#pending = undefined;
  }

  #enqueue<T>(operation: () => T | Promise<T>, fallback: T): T | Promise<T> {
    const run = () => {
      if (this.#disposed) return fallback;
      try {
        return operation();
      } catch (error) {
        this.fail(error);
        return fallback;
      }
    };
    const result = this.#tail ? this.#tail.then(run) : run();
    if (!(result instanceof Promise)) return result;
    const guarded = result.catch((error: unknown) => {
      this.fail(error);
      return fallback;
    });
    const tail = guarded.finally(() => {
      if (this.#tail === tail) this.#tail = undefined;
    });
    this.#tail = tail;
    return guarded;
  }
}
