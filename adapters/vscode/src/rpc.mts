import {
  type ClientNotificationMap,
  type CustomRequestMap,
  type JsonRpcMessageEnvelope,
  type JsonRpcResponseEnvelope,
  type JsonValue,
  type LspMethod,
  type RpcId,
  type ServerNotificationMap,
  type Validator,
  clientNotificationValidators,
  customRequestParamsValidators,
  customRequestResultValidators,
  isCustomRequestMethod,
  isJsonRpcMessageEnvelope,
  isJsonRpcResponseEnvelope,
  isServerNotificationMethod,
  serverNotificationValidators,
} from "@fleximark/contracts/protocol";
import { EventEmitter } from "node:events";
import type { Readable, Writable } from "node:stream";

interface PendingRequest {
  method: string;
  accept(value: unknown): boolean;
  reject: (reason: Error) => void;
  timer: NodeJS.Timeout;
}

type Envelope =
  | { kind: "response"; value: JsonRpcResponseEnvelope }
  | { kind: "message"; value: JsonRpcMessageEnvelope }
  | { kind: "invalid" };

function decodeEnvelope(value: unknown): Envelope {
  if (isJsonRpcMessageEnvelope(value)) return { kind: "message", value };
  if (isJsonRpcResponseEnvelope(value)) return { kind: "response", value };
  return { kind: "invalid" };
}

export class JsonRpcResponseError extends Error {
  constructor(
    message: string,
    readonly code: number,
    readonly data?: JsonValue,
  ) {
    super(`${message} (${code})`);
  }
}

export class JsonRpcConnection extends EventEmitter {
  readonly #input: Readable;
  readonly #output: Writable;
  readonly #onData: (chunk: Buffer | string) => void;
  readonly #onInputError: (error: Error) => void;
  readonly #onInputEnd: () => void;
  readonly #onOutputError: (error: Error) => void;
  readonly #pending = new Map<number, PendingRequest>();
  #buffer = Buffer.alloc(0);
  #nextId = 1;
  #closed = false;

  onNotification<Method extends keyof ServerNotificationMap>(
    method: Method,
    listener: (params: ServerNotificationMap[Method]) => void,
  ): void {
    this.on(method, listener);
  }

  constructor(input: Readable, output: Writable) {
    super();
    this.#input = input;
    this.#output = output;
    this.#onData = (chunk) => {
      if (this.#closed) return;
      this.#buffer = Buffer.concat([
        this.#buffer,
        Buffer.isBuffer(chunk) ? chunk : Buffer.from(chunk),
      ]);
      this.#readFrames();
    };
    this.#onInputError = (error) => this.close(error);
    this.#onInputEnd = () => this.close(new Error("FlexiMark daemon exited"));
    this.#onOutputError = (error) => this.close(error);
    input.on("data", this.#onData);
    input.on("error", this.#onInputError);
    input.on("end", this.#onInputEnd);
    output.on("error", this.#onOutputError);
  }

  get closed(): boolean {
    return this.#closed;
  }

  request<Method extends keyof CustomRequestMap>(
    method: Method,
    params: CustomRequestMap[Method]["params"],
    timeoutMs?: number,
    receive?: (result: CustomRequestMap[Method]["result"]) => void,
  ): Promise<CustomRequestMap[Method]["result"]> {
    const validator = customRequestParamsValidators[method];
    if (!validator(params))
      return Promise.reject(new Error(`Invalid params for ${method}`));
    return this.#request(
      method,
      params,
      customRequestResultValidators[method],
      timeoutMs,
      receive,
    );
  }

  requestLsp<Result>(
    method: LspMethod,
    params?: object | JsonValue[],
    timeoutMs?: number,
  ): Promise<Result> {
    if (method.startsWith("fleximark/") || isCustomRequestMethod(method))
      return Promise.reject(
        new Error(`Custom method ${method} must use the typed request API`),
      );
    return this.#request<Result>(method, params, undefined, timeoutMs);
  }

  notify<Method extends keyof ClientNotificationMap>(
    method: Method,
    params: ClientNotificationMap[Method],
  ): void {
    const validator = clientNotificationValidators[method];
    if (!validator(params)) throw new Error(`Invalid params for ${method}`);
    if (!this.#closed) this.#write({ jsonrpc: "2.0", method, params });
  }

  notifyLsp(method: LspMethod, params?: object | JsonValue[]): void {
    if (method.startsWith("fleximark/"))
      throw new Error(`Custom method ${method} must use the typed notify API`);
    if (!this.#closed) this.#write({ jsonrpc: "2.0", method, params });
  }

  respond(
    id: RpcId,
    result?: JsonValue,
    error?: JsonRpcResponseEnvelope["error"],
  ): void {
    if (!this.#closed) this.#write({ jsonrpc: "2.0", id, result, error });
  }

  close(reason = new Error("JSON-RPC connection closed")): void {
    if (this.#closed) return;
    this.#closed = true;
    this.#buffer = Buffer.alloc(0);
    this.#input.off("data", this.#onData);
    this.#input.off("end", this.#onInputEnd);
    for (const pending of this.#pending.values()) {
      clearTimeout(pending.timer);
      pending.reject(reason);
    }
    this.#pending.clear();
    this.emit("close", reason);
  }

  #request<Result>(
    method: string,
    params: object | JsonValue[] | undefined,
    validateResult: Validator<Result> | undefined,
    timeoutMs = 15_000,
    receive?: (value: Result) => void,
  ): Promise<Result> {
    if (this.#closed)
      return Promise.reject(new Error("JSON-RPC connection is closed"));
    const id = this.#nextId++;
    return new Promise<Result>((resolve, reject) => {
      const timer = setTimeout(() => {
        this.#pending.delete(id);
        reject(new Error(`JSON-RPC request timed out: ${method}`));
      }, timeoutMs);
      this.#pending.set(id, {
        method,
        accept: (value) => {
          const complete = (result: Result) => {
            this.#pending.delete(id);
            clearTimeout(timer);
            try {
              receive?.(result);
              resolve(result);
            } catch (error) {
              reject(error instanceof Error ? error : new Error(String(error)));
            }
          };
          if (validateResult) {
            if (!validateResult(value)) return false;
            complete(value);
          } else {
            // Standard LSP methods are the explicit untyped transport boundary.
            complete(value as Result);
          }
          return true;
        },
        reject,
        timer,
      });
      this.#write({ jsonrpc: "2.0", id, method, params });
    });
  }

  #write(message: JsonRpcMessageEnvelope | JsonRpcResponseEnvelope): void {
    const body = Buffer.from(JSON.stringify(message));
    this.#output.write(`Content-Length: ${body.length}\r\n\r\n`);
    this.#output.write(body);
  }

  #readFrames(): void {
    if (this.#closed) return;
    while (true) {
      if (this.#closed) return;
      const headerEnd = this.#buffer.indexOf("\r\n\r\n");
      if (headerEnd < 0) {
        if (this.#buffer.length > 8 * 1024)
          this.close(new Error("Invalid JSON-RPC frame: header is too large"));
        return;
      }
      if (headerEnd > 8 * 1024) {
        this.close(new Error("Invalid JSON-RPC frame: header is too large"));
        return;
      }
      const header = this.#buffer.subarray(0, headerEnd).toString("ascii");
      const matches = [...header.matchAll(/^Content-Length:\s*(\d+)\s*$/gim)];
      if (matches.length !== 1) {
        this.close(new Error("Invalid JSON-RPC frame: missing Content-Length"));
        return;
      }
      const length = Number(matches[0][1]);
      if (!Number.isSafeInteger(length) || length < 0) {
        this.close(new Error("Invalid JSON-RPC frame length"));
        return;
      }
      const bodyStart = headerEnd + 4;
      if (this.#buffer.length < bodyStart + length) return;
      const body = this.#buffer.subarray(bodyStart, bodyStart + length);
      this.#buffer = this.#buffer.subarray(bodyStart + length);
      try {
        const text = new TextDecoder("utf-8", { fatal: true }).decode(body);
        const envelope = decodeEnvelope(JSON.parse(text) as unknown);
        if (envelope.kind === "invalid")
          throw new Error("Invalid JSON-RPC envelope");
        this.#dispatch(envelope);
      } catch (error) {
        this.close(error instanceof Error ? error : new Error(String(error)));
        return;
      }
    }
  }

  #dispatch(envelope: Exclude<Envelope, { kind: "invalid" }>): void {
    if (this.#closed) return;
    if (envelope.kind === "response") {
      const message = envelope.value;
      if (typeof message.id !== "number") return;
      const pending = this.#pending.get(message.id);
      if (!pending) return;
      if (message.error) {
        this.#pending.delete(message.id);
        clearTimeout(pending.timer);
        pending.reject(
          new JsonRpcResponseError(
            message.error.message,
            message.error.code,
            message.error.data,
          ),
        );
        return;
      }
      if (pending.accept(message.result)) return;
      this.close(new Error(`Invalid protocol result for ${pending.method}`));
      return;
    }

    const message = envelope.value;
    if (message.method === "window/showDocument") {
      this.emit("request", message);
      return;
    }
    if (!isServerNotificationMethod(message.method)) return;
    const validator = serverNotificationValidators[message.method];
    if (!validator(message.params)) {
      this.emit("invalidMessage", message);
      return;
    }
    this.emit("message", message);
    this.emit(message.method, message.params);
  }
}
