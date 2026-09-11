import { EventEmitter } from "node:events";
import type { Readable, Writable } from "node:stream";

import {
  type ClientNotificationMap,
  type CustomRequestMap,
  type JsonValue,
  type LspMethod,
  type Validator,
  clientNotificationValidators,
  customRequestParamsValidators,
  customRequestResultValidators,
  isCustomRequestMethod,
  isJsonRpcMessageEnvelope,
  isJsonRpcResponseEnvelope,
  isServerNotificationMethod,
  serverNotificationValidators,
} from "./protocol.mjs";

export interface JsonRpcResponse {
  jsonrpc: "2.0";
  id: number | string;
  result?: JsonValue;
  error?: { code: number; message: string; data?: JsonValue };
}

export interface JsonRpcRequest {
  jsonrpc: "2.0";
  id?: number | string;
  method: string;
  params?: object | JsonValue[];
}

interface PendingRequest {
  method: string;
  validateResult?: Validator<unknown>;
  resolve: (value: unknown) => void;
  reject: (reason: Error) => void;
  timer: NodeJS.Timeout;
}

type Envelope =
  | { kind: "response"; value: JsonRpcResponse }
  | { kind: "message"; value: JsonRpcRequest }
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
  ): Promise<CustomRequestMap[Method]["result"]> {
    const validator = customRequestParamsValidators[
      method
    ] as Validator<unknown>;
    if (!validator(params))
      return Promise.reject(new Error(`Invalid params for ${method}`));
    return this.#request(
      method,
      params,
      customRequestResultValidators[method] as Validator<unknown>,
      timeoutMs,
    ) as Promise<CustomRequestMap[Method]["result"]>;
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
    return this.#request(
      method,
      params,
      undefined,
      timeoutMs,
    ) as Promise<Result>;
  }

  notify<Method extends keyof ClientNotificationMap>(
    method: Method,
    params: ClientNotificationMap[Method],
  ): void {
    const validator = clientNotificationValidators[
      method
    ] as Validator<unknown>;
    if (!validator(params)) throw new Error(`Invalid params for ${method}`);
    if (!this.#closed) this.#write({ jsonrpc: "2.0", method, params });
  }

  notifyLsp(method: LspMethod, params?: object | JsonValue[]): void {
    if (method.startsWith("fleximark/"))
      throw new Error(`Custom method ${method} must use the typed notify API`);
    if (!this.#closed) this.#write({ jsonrpc: "2.0", method, params });
  }

  respond(
    id: number | string,
    result?: JsonValue,
    error?: JsonRpcResponse["error"],
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

  #request(
    method: string,
    params: object | JsonValue[] | undefined,
    validateResult: Validator<unknown> | undefined,
    timeoutMs = 15_000,
  ): Promise<unknown> {
    if (this.#closed)
      return Promise.reject(new Error("JSON-RPC connection is closed"));
    const id = this.#nextId++;
    return new Promise((resolve, reject) => {
      const timer = setTimeout(() => {
        this.#pending.delete(id);
        reject(new Error(`JSON-RPC request timed out: ${method}`));
      }, timeoutMs);
      this.#pending.set(id, { method, validateResult, resolve, reject, timer });
      this.#write({ jsonrpc: "2.0", id, method, params });
    });
  }

  #write(message: JsonRpcRequest | JsonRpcResponse): void {
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
      if (!pending.validateResult || pending.validateResult(message.result)) {
        this.#pending.delete(message.id);
        clearTimeout(pending.timer);
        pending.resolve(message.result);
        return;
      }
      this.close(new Error(`Invalid protocol result for ${pending.method}`));
      return;
    }

    const message = envelope.value;
    if (!isServerNotificationMethod(message.method)) return;
    const validator = serverNotificationValidators[
      message.method
    ] as Validator<unknown>;
    if (!validator(message.params)) {
      this.emit("invalidMessage", message);
      return;
    }
    this.emit("message", message);
  }
}
