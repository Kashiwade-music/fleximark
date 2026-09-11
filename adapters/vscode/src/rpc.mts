import { EventEmitter } from "node:events";
import type { Readable, Writable } from "node:stream";

type JsonValue =
  null | boolean | number | string | JsonValue[] | { [key: string]: JsonValue };

export interface JsonRpcResponse {
  jsonrpc: "2.0";
  id: number;
  result?: JsonValue;
  error?: { code: number; message: string; data?: JsonValue };
}

export interface JsonRpcRequest {
  jsonrpc: "2.0";
  id?: number;
  method: string;
  params?: object;
}

interface PendingRequest {
  resolve: (value: JsonValue | undefined) => void;
  reject: (reason: Error) => void;
  timer: NodeJS.Timeout;
}

export class JsonRpcConnection extends EventEmitter {
  readonly #output: Writable;
  readonly #pending = new Map<number, PendingRequest>();
  #buffer = Buffer.alloc(0);
  #nextId = 1;
  #closed = false;

  constructor(input: Readable, output: Writable) {
    super();
    this.#output = output;
    input.on("data", (chunk: Buffer | string) => {
      this.#buffer = Buffer.concat([
        this.#buffer,
        Buffer.isBuffer(chunk) ? chunk : Buffer.from(chunk),
      ]);
      this.#readFrames();
    });
    input.on("error", (error) => this.close(error));
    input.on("end", () => this.close(new Error("FlexiMark daemon exited")));
    output.on("error", (error) => this.close(error));
  }

  request<T>(method: string, params?: object, timeoutMs = 15_000): Promise<T> {
    if (this.#closed)
      return Promise.reject(new Error("JSON-RPC connection is closed"));
    const id = this.#nextId++;
    return new Promise<T>((resolve, reject) => {
      const timer = setTimeout(() => {
        this.#pending.delete(id);
        reject(new Error(`JSON-RPC request timed out: ${method}`));
      }, timeoutMs);
      this.#pending.set(id, {
        resolve: resolve as (value: JsonValue | undefined) => void,
        reject,
        timer,
      });
      this.#write({ jsonrpc: "2.0", id, method, params });
    });
  }

  notify(method: string, params?: object): void {
    if (!this.#closed) this.#write({ jsonrpc: "2.0", method, params });
  }

  respond(
    id: number,
    result?: JsonValue,
    error?: JsonRpcResponse["error"],
  ): void {
    if (!this.#closed) this.#write({ jsonrpc: "2.0", id, result, error });
  }

  close(reason = new Error("JSON-RPC connection closed")): void {
    if (this.#closed) return;
    this.#closed = true;
    for (const pending of this.#pending.values()) {
      clearTimeout(pending.timer);
      pending.reject(reason);
    }
    this.#pending.clear();
    this.emit("close", reason);
  }

  #write(message: JsonRpcRequest | JsonRpcResponse): void {
    const body = Buffer.from(JSON.stringify(message));
    this.#output.write(`Content-Length: ${body.length}\r\n\r\n`);
    this.#output.write(body);
  }

  #readFrames(): void {
    while (true) {
      const headerEnd = this.#buffer.indexOf("\r\n\r\n");
      if (headerEnd < 0) return;
      const header = this.#buffer.subarray(0, headerEnd).toString("ascii");
      const match = /^Content-Length:\s*(\d+)$/im.exec(header);
      if (!match) {
        this.close(new Error("Invalid JSON-RPC frame: missing Content-Length"));
        return;
      }
      const length = Number(match[1]);
      if (
        !Number.isSafeInteger(length) ||
        length < 0 ||
        length > 16 * 1024 * 1024
      ) {
        this.close(new Error("Invalid JSON-RPC frame length"));
        return;
      }
      const bodyStart = headerEnd + 4;
      if (this.#buffer.length < bodyStart + length) return;
      const body = this.#buffer.subarray(bodyStart, bodyStart + length);
      this.#buffer = this.#buffer.subarray(bodyStart + length);
      try {
        this.#dispatch(
          JSON.parse(body.toString("utf8")) as JsonRpcRequest | JsonRpcResponse,
        );
      } catch (error) {
        this.close(error instanceof Error ? error : new Error(String(error)));
        return;
      }
    }
  }

  #dispatch(message: JsonRpcRequest | JsonRpcResponse): void {
    if (!("method" in message)) {
      const pending = this.#pending.get(message.id);
      if (!pending) return;
      this.#pending.delete(message.id);
      clearTimeout(pending.timer);
      if (message.error) {
        pending.reject(
          new Error(`${message.error.message} (${message.error.code})`),
        );
      } else {
        pending.resolve(message.result);
      }
      return;
    }
    this.emit("message", message);
  }
}
