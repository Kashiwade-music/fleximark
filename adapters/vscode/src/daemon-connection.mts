import type { JsonRpcConnection } from "./rpc.mjs";

/** Owns one daemon incarnation and invalidates all of its asynchronous work. */
export class DaemonConnection {
  readonly #abort = new AbortController();
  #daemonInstanceId: string;

  constructor(
    readonly rpc: JsonRpcConnection,
    readonly generation: number,
    daemonInstanceId = "",
  ) {
    this.#daemonInstanceId = daemonInstanceId;
    rpc.once("close", () => this.dispose());
  }

  get daemonInstanceId(): string {
    return this.#daemonInstanceId;
  }
  get signal(): AbortSignal {
    return this.#abort.signal;
  }

  identify(daemonInstanceId: string): void {
    if (this.#daemonInstanceId && this.#daemonInstanceId !== daemonInstanceId)
      throw new Error("Daemon connection identity cannot change");
    this.#daemonInstanceId = daemonInstanceId;
  }

  dispose(reason?: Error): void {
    if (this.signal.aborted) return;
    this.#abort.abort(reason);
    this.rpc.close(reason);
  }
}

/** Fences a document or preview operation against its connection and owner. */
export class ConnectionBinding {
  constructor(
    readonly connection: DaemonConnection,
    private readonly current: () => boolean,
  ) {}
  isCurrent(connection = this.connection): boolean {
    return (
      connection === this.connection &&
      !this.connection.signal.aborted &&
      !this.connection.rpc.closed &&
      this.current()
    );
  }
}
