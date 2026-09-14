import * as assert from "node:assert/strict";
import { PassThrough } from "node:stream";

import type { LspMethod } from "../../adapters/vscode/src/protocol.mjs";
import { JsonRpcConnection } from "../../adapters/vscode/src/rpc.mjs";

function frame(message: unknown): Buffer {
  const body = Buffer.from(JSON.stringify(message));
  return Buffer.concat([
    Buffer.from(`Content-Length: ${body.length}\r\n\r\n`),
    body,
  ]);
}

const EMPTY_DIAGNOSTICS = {
  jsonrpc: "2.0",
  method: "textDocument/publishDiagnostics",
  params: { uri: "file:///document.md", diagnostics: [] },
};

function rpcHarness() {
  const daemonOutput = new PassThrough();
  const daemonInput = new PassThrough();
  const connection = new JsonRpcConnection(daemonOutput, daemonInput);
  const messages: unknown[] = [];
  const outgoing: Buffer[] = [];
  connection.on("message", (message) => messages.push(message));
  daemonInput.on("data", (chunk: Buffer) => outgoing.push(chunk));
  return { connection, daemonInput, daemonOutput, messages, outgoing };
}

async function assertInboundState(message: unknown, closed: boolean) {
  const { connection, daemonOutput, messages } = rpcHarness();
  daemonOutput.write(frame(message));
  await new Promise<void>((resolve) => setImmediate(resolve));
  assert.equal(connection.closed, closed);
  assert.deepEqual(messages, []);
  connection.close();
}

export const suiteName = "JSON-RPC transport";

export function suite(): void {
  test("reads split Content-Length frames and resolves requests", async () => {
    const { connection, daemonOutput, outgoing } = rpcHarness();

    const pending = connection.requestLsp<{ ok: boolean }>("workspace/test", {
      value: 1,
    });
    await new Promise<void>((resolve) => setImmediate(resolve));
    const requestText = Buffer.concat(outgoing).toString("utf8");
    const body = JSON.stringify({
      jsonrpc: "2.0",
      id: 1,
      result: { ok: true },
    });
    const response = Buffer.from(
      `Content-Length: ${Buffer.byteLength(body)}\r\n\r\n${body}`,
    );
    daemonOutput.write(response.subarray(0, 11));
    daemonOutput.write(response.subarray(11));

    assert.match(requestText, /"method":"workspace\/test"/);
    assert.deepEqual(await pending, { ok: true });
    connection.close();
  });

  test("close is idempotent and permanently suppresses later writes", async () => {
    const { connection, outgoing } = rpcHarness();
    let closeEvents = 0;
    connection.on("close", () => (closeEvents += 1));
    const pending = connection.requestLsp("shutdown");
    await new Promise<void>((resolve) => setImmediate(resolve));
    const bytesBeforeClose = Buffer.concat(outgoing);

    connection.close(new Error("stopped"));
    connection.close(new Error("late close"));
    await assert.rejects(pending, /^Error: stopped$/);
    connection.notifyLsp("exit");
    connection.respond(7, null);
    await assert.rejects(
      connection.requestLsp("shutdown"),
      /^Error: JSON-RPC connection is closed$/,
    );
    await new Promise<void>((resolve) => setImmediate(resolve));

    assert.equal(closeEvents, 1);
    assert.deepEqual(Buffer.concat(outgoing), bytesBeforeClose);
  });

  test("preserves valid notification and error response wire bytes", async () => {
    const { connection, outgoing } = rpcHarness();
    const previewEvent = {
      daemonInstanceId: "daemon",
      previewSessionId: "preview",
      renderRevision: 1,
      event: {
        type: "selectNode",
        previewSessionId: "preview",
        renderRevision: 1,
        nodeId: "日本語",
      },
    } as const;
    const responseError = {
      code: -32801,
      message: "content modified",
      data: { expectedDocumentVersion: 4 },
    };
    connection.notify("fleximark/previewEvent", previewEvent);
    connection.respond(7, undefined, responseError);
    await new Promise<void>((resolve) => setImmediate(resolve));

    assert.deepEqual(
      Buffer.concat(outgoing),
      Buffer.concat([
        frame({
          jsonrpc: "2.0",
          method: "fleximark/previewEvent",
          params: previewEvent,
        }),
        frame({ jsonrpc: "2.0", id: 7, error: responseError }),
      ]),
    );
    connection.close();
  });

  test("dispatches multiple frames from one chunk in wire order", async () => {
    const { connection, daemonOutput, messages } = rpcHarness();
    const first = {
      jsonrpc: "2.0",
      method: "fleximark/requestFullText",
      params: {
        daemonInstanceId: "daemon",
        uri: "file:///document.md",
        documentSessionId: "document",
        reason: "contentHashMismatch",
      },
    };
    const second = EMPTY_DIAGNOSTICS;

    daemonOutput.write(Buffer.concat([frame(first), frame(second)]));
    await new Promise<void>((resolve) => setImmediate(resolve));

    assert.deepEqual(messages, [first, second]);
    assert.equal(connection.closed, false);
    connection.close();
  });

  test("preserves JSON-RPC error code, message, and data", async () => {
    const { connection, daemonOutput } = rpcHarness();
    const pending = connection.request("fleximark/changeDocument", {
      daemonInstanceId: "daemon",
      documentSessionId: "document",
      baseDocumentVersion: 3,
      baseContentHash: "before",
      documentVersion: 4,
      text: "# After\n",
    });

    daemonOutput.write(
      frame({
        jsonrpc: "2.0",
        id: 1,
        error: {
          code: -32801,
          message: "content modified",
          data: { expectedDocumentVersion: 4 },
        },
      }),
    );

    await assert.rejects(pending, (error: unknown) => {
      assert.ok(error instanceof Error);
      assert.equal(error.name, "Error");
      assert.equal(error.message, "content modified (-32801)");
      assert.equal((error as { code?: unknown }).code, -32801);
      assert.deepEqual((error as { data?: unknown }).data, {
        expectedDocumentVersion: 4,
      });
      return true;
    });
    assert.equal(connection.closed, false);
    connection.close();
  });

  test("closes on malformed JSON without dispatching a message", async () => {
    const { connection, daemonOutput, messages } = rpcHarness();
    let closeReason: unknown;
    connection.on("close", (reason) => {
      closeReason = reason;
    });
    const body = Buffer.from('{"jsonrpc":"2.0","method":');

    daemonOutput.write(
      Buffer.concat([
        Buffer.from(`Content-Length: ${body.length}\r\n\r\n`),
        body,
      ]),
    );
    await new Promise<void>((resolve) => setImmediate(resolve));

    assert.equal(connection.closed, true);
    assert.deepEqual(messages, []);
    assert.ok(closeReason instanceof SyntaxError);
  });

  test("ignores a late or unknown response id and keeps the connection open", async () => {
    await assertInboundState(
      { jsonrpc: "2.0", id: 999, result: { ignored: true } },
      false,
    );
  });

  test("closes on a malformed JSON-RPC envelope without dispatch", async () => {
    await assertInboundState(
      {
        jsonrpc: "1.0",
        method: "fleximark/requestFullText",
        params: {},
      },
      true,
    );
  });

  test("ignores unknown notifications with array params and stays open", async () => {
    await assertInboundState(
      { jsonrpc: "2.0", method: "unknown/notification", params: [] },
      false,
    );
  });

  test("closes on an envelope with primitive params", async () => {
    await assertInboundState(
      {
        jsonrpc: "2.0",
        method: "unknown/notification",
        params: "not structured",
      },
      true,
    );
  });

  test("rejects and closes on a malformed typed method result", async () => {
    const { connection, daemonOutput } = rpcHarness();
    const pending = connection.request("fleximark/initialize", {
      protocolVersion: 1,
      client: { name: "boundary-test", version: "1" },
    });

    daemonOutput.write(
      frame({
        jsonrpc: "2.0",
        id: 1,
        result: {
          protocolVersion: 1,
          daemonInstanceId: "daemon",
          workspaceStatuses: [{ uri: "file:///workspace", enabled: "yes" }],
          capabilities: {},
        },
      }),
    );

    await assert.rejects(pending, /invalid|malformed|protocol/i);
    assert.equal(connection.closed, true);
  });

  test("ignores malformed known notifications while keeping the connection open", async () => {
    await assertInboundState(
      {
        jsonrpc: "2.0",
        method: "fleximark/requestFullText",
        params: {
          daemonInstanceId: "daemon",
          uri: "file:///document.md",
          documentSessionId: "document",
          reason: 7,
        },
      },
      false,
    );
  });

  test("rejects createPreview results whose nested session does not match", async () => {
    const { connection, daemonOutput, messages } = rpcHarness();
    const pending = connection.request("fleximark/createPreview", {
      daemonInstanceId: "daemon",
      documentSessionId: "document",
      expectedDocumentVersion: 1,
      target: "embeddedHtml",
    });
    daemonOutput.write(
      Buffer.concat([
        frame({
          jsonrpc: "2.0",
          id: 1,
          result: {
            previewSessionId: "outer",
            initialPublication: {
              type: "full",
              previewSessionId: "inner",
              documentVersion: 1,
              resultRenderRevision: 1,
              rendererFingerprint: "sha256:renderer",
              nodeIds: ["document-root"],
              navigation: [],
              style: null,
              assets: [],
              html: '<main data-fleximark-node-id="document-root"></main>',
            },
          },
        }),
        frame(EMPTY_DIAGNOSTICS),
      ]),
    );

    await assert.rejects(pending, /invalid protocol result/i);
    assert.equal(connection.closed, true);
    assert.deepEqual(messages, []);
  });

  test("a closed connection discards buffered and later input permanently", async () => {
    const { connection, daemonOutput, messages } = rpcHarness();
    const valid = frame(EMPTY_DIAGNOSTICS);

    daemonOutput.write(
      Buffer.concat([Buffer.from("Content-Length: 16777217\r\n\r\n"), valid]),
    );
    daemonOutput.write(valid);
    await new Promise<void>((resolve) => setImmediate(resolve));

    assert.equal(connection.closed, true);
    assert.equal(daemonOutput.listenerCount("data"), 0);
    assert.deepEqual(messages, []);
  });

  test("late input and output errors are absorbed after close", () => {
    const { connection, daemonInput, daemonOutput } = rpcHarness();
    connection.close();

    assert.doesNotThrow(() =>
      daemonOutput.emit("error", new Error("late input")),
    );
    assert.doesNotThrow(() =>
      daemonInput.emit("error", new Error("late output")),
    );
    assert.equal(connection.closed, true);
  });

  test("LSP escape hatches retain runtime guards for cast custom methods", async () => {
    const { connection } = rpcHarness();
    await assert.rejects(
      connection.requestLsp("fleximark/render" as LspMethod),
      /must use the typed request API/,
    );
    assert.throws(
      () => connection.notifyLsp("fleximark/previewEvent" as LspMethod),
      /must use the typed notify API/,
    );
    connection.close();
  });
}
