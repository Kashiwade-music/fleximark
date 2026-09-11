import * as assert from "node:assert/strict";
import { PassThrough } from "node:stream";

import { JsonRpcConnection } from "../../adapters/vscode/src/rpc.mjs";

export const suiteName = "JSON-RPC transport";

export function suite(): void {
  test("reads split Content-Length frames and resolves requests", async () => {
    const daemonOutput = new PassThrough();
    const daemonInput = new PassThrough();
    const connection = new JsonRpcConnection(daemonOutput, daemonInput);
    const outgoing: Buffer[] = [];
    daemonInput.on("data", (chunk: Buffer) => outgoing.push(chunk));

    const pending = connection.request<{ ok: boolean }>("fleximark/test", {
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

    assert.match(requestText, /"method":"fleximark\/test"/);
    assert.deepEqual(await pending, { ok: true });
    connection.close();
  });

  test("rejects pending work when the connection closes", async () => {
    const daemonOutput = new PassThrough();
    const daemonInput = new PassThrough();
    const connection = new JsonRpcConnection(daemonOutput, daemonInput);
    const pending = connection.request("fleximark/test");
    connection.close(new Error("gone"));
    await assert.rejects(pending, /gone/);
    assert.equal(connection.closed, true);
  });
}
