import type { RenderFrame } from "@fleximark/contracts/protocol";
import { DaemonConnection } from "fleximark/src/daemon-connection";
import {
  PreviewController,
  type PreviewEnvironment,
} from "fleximark/src/preview-controller";
import { JsonRpcConnection } from "fleximark/src/rpc";
import type { DocumentRuntime } from "fleximark/src/runtime-state";
import { isPreviewHostMessage } from "fleximark/webview/bridge";
import { PassThrough } from "node:stream";
import type * as vscode from "vscode";

import { setupRuntimeVscode } from "./vscode-harness.mjs";
import { RuntimePanel, Uri, __test } from "./vscode-pure-stub.mjs";

export interface WireRequest {
  id: number;
  method: string;
  params: Record<string, unknown>;
}

export function renderFrame(revision = 1, session = "preview"): RenderFrame {
  return {
    previewSessionId: session,
    documentVersion: 1,
    renderRevision: revision,
    rendererFingerprint: "a".repeat(64),
    style: null,
    assets: [],
    blocks: [
      {
        id: "node",
        nodeIds: ["node"],
        html: '<p data-fleximark-node-id="node">frame</p>',
      },
    ],
    navigation: [
      {
        nodeId: "node",
        depth: 0,
        sourceRange: {
          byteStart: 0,
          byteEnd: 1,
          start: { line: 0, character: 0, encoding: "utf16" },
          end: { line: 0, character: 1, encoding: "utf16" },
        },
      },
    ],
  };
}

export function deliveredFrames(panel: RuntimePanel): RenderFrame[] {
  return panel.messages.flatMap((message) =>
    isPreviewHostMessage(message) && message.type === "previewFrame"
      ? [message.frame]
      : [],
  );
}

/** Drives public controller methods through a real framed RPC connection. */
export class PreviewHarness {
  readonly vscode = setupRuntimeVscode(process.cwd());
  readonly runtime: DocumentRuntime = {
    removed: false,
    workspace: this.vscode.workspace,
    documents: new Map([
      [
        this.vscode.document.uri.toString(),
        { sessionId: "document", version: 1 },
      ],
    ]),
    previews: new Map(),
  };
  readonly runtimes = new Map([["root", this.runtime]]);
  readonly requests: WireRequest[] = [];
  readonly errors: unknown[] = [];
  readonly held = new Set<string>();
  readonly pending: {
    request: WireRequest;
    respond: (result: unknown, extra?: unknown[]) => void;
  }[] = [];
  readonly frames = new Map<string, RenderFrame>();
  readonly controller: PreviewController;
  connection: DaemonConnection | undefined;
  destination = this.runtime;
  onRequest?: (
    request: WireRequest,
    respond: (result: unknown, extra?: unknown[]) => void,
  ) => boolean;
  #input = new PassThrough();
  #generation = 0;
  #preview = 0;

  constructor() {
    const connection = () => this.connection;
    const environment: PreviewEnvironment = {
      runtimes: this.runtimes,
      get connection() {
        return connection();
      },
      start: async () => this.destination,
      syncDocument: async (document) => {
        if (!this.destination.documents.has(document.uri.toString()))
          this.destination.documents.set(document.uri.toString(), {
            sessionId: "document-next",
            version: document.version,
          });
      },
      report: (error) => {
        this.errors.push(error);
      },
      log: () => undefined,
    };
    this.controller = new PreviewController(
      environment,
      this.vscode.context.extensionUri,
    );
    this.replaceConnection();
  }

  get activeConnection(): DaemonConnection {
    if (!this.connection) throw new Error("No active connection");
    return this.connection;
  }

  replaceConnection(): DaemonConnection {
    this.connection?.dispose();
    this.#input = new PassThrough();
    const input = this.#input;
    const output = new PassThrough();
    const connection = new DaemonConnection(
      new JsonRpcConnection(input, output),
      ++this.#generation,
      `daemon-${this.#generation}`,
    );
    this.connection = connection;
    connection.rpc.onNotification("fleximark/previewChanged", (params) =>
      this.controller.frame(connection, params),
    );
    connection.rpc.onNotification("fleximark/previewEvent", (params) => {
      void this.controller.event(connection, params);
    });
    let bytes = Buffer.alloc(0);
    output.on("data", (chunk: Buffer) => {
      bytes = Buffer.concat([bytes, chunk]);
      for (;;) {
        const end = bytes.indexOf("\r\n\r\n");
        if (end < 0) return;
        const length = Number(
          /Content-Length: (\d+)/i.exec(bytes.subarray(0, end).toString())?.[1],
        );
        if (bytes.length < end + 4 + length) return;
        const request = JSON.parse(
          bytes.subarray(end + 4, end + 4 + length).toString(),
        ) as WireRequest;
        bytes = bytes.subarray(end + 4 + length);
        this.requests.push(request);
        if (request.id === undefined) continue;
        const respond = (result: unknown, extra: unknown[] = []) =>
          this.write(input, [
            { jsonrpc: "2.0", id: request.id, result },
            ...extra,
          ]);
        if (this.held.has(request.method)) {
          this.pending.push({ request, respond });
          continue;
        }
        if (this.onRequest?.(request, respond)) continue;
        if (request.method === "fleximark/createPreview") {
          const id = `preview-${++this.#preview}`;
          const frame = renderFrame(1, id);
          this.frames.set(id, frame);
          respond({
            previewSessionId: id,
            frame,
            ...(request.params.target === "externalBrowser"
              ? { url: `http://127.0.0.1:12345/${id}` }
              : {}),
          });
        } else if (request.method === "fleximark/rerenderPreview") {
          const id = String(request.params.previewSessionId);
          const previous = this.frames.get(id);
          const frame = renderFrame((previous?.renderRevision ?? 0) + 1, id);
          this.frames.set(id, frame);
          respond(null, [this.changed(frame, connection)]);
        } else respond(null);
      }
    });
    return connection;
  }

  changed(frame: RenderFrame, connection = this.connection) {
    return {
      jsonrpc: "2.0",
      method: "fleximark/previewChanged",
      params: {
        daemonInstanceId: connection?.daemonInstanceId,
        previewSessionId: frame.previewSessionId,
        renderRevision: frame.renderRevision,
        frame,
      },
    };
  }

  push(frame: RenderFrame): void {
    this.frames.set(frame.previewSessionId, frame);
    this.write(this.#input, [this.changed(frame)]);
  }

  write(input: PassThrough, messages: unknown[]): void {
    input.write(
      Buffer.concat(
        messages.map((message) => {
          const body = Buffer.from(JSON.stringify(message));
          return Buffer.concat([
            Buffer.from(`Content-Length: ${body.length}\r\n\r\n`),
            body,
          ]);
        }),
      ),
    );
  }

  async open() {
    await this.controller.open("embeddedHtml");
    const panel = this.vscode.panels.at(-1);
    if (!panel) throw new Error("No preview panel");
    const preview = [...this.runtime.previews.values()].at(-1);
    if (!preview || preview.target !== "embeddedHtml")
      throw new Error("No embedded preview");
    return { panel, preview };
  }

  nextDocument(uri = "file:///detached.md") {
    const document = {
      ...this.vscode.document,
      uri: new Uri(uri),
      fileName: uri,
    };
    const editor = { ...this.vscode.editor, document };
    __test.activeTextEditor = editor;
    __test.textDocuments.push(document);
    return editor as typeof editor & vscode.TextEditor;
  }

  dispose(): void {
    this.connection?.dispose();
    this.controller.dispose();
    this.vscode.reset();
  }
}
