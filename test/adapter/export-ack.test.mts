import * as assert from "node:assert/strict";

import { openCommandResult } from "../../adapters/vscode/src/adapter.mjs";

export const suiteName = "Export acknowledgement";

export function suite(): void {
  const params = {
    daemonInstanceId: "daemon",
    command: "exportHtml",
    documentSessionId: "document",
    expectedDocumentVersion: 3,
    workspaceUri: "file:///workspace",
  };

  test("acknowledges if and only if the exported URI opens", async () => {
    const calls: string[] = [];
    await openCommandResult(
      params,
      { openUri: "file:///workspace/public/index.html" },
      async () => {
        calls.push("open");
      },
      async (request) => {
        calls.push(request.command);
        return {};
      },
    );
    assert.deepEqual(calls, ["open", "acknowledgeExport"]);
    let acknowledged = false;
    await assert.rejects(
      openCommandResult(
        params,
        { openUri: "file:///workspace/public/index.html" },
        async () => {
          throw new Error("open failed");
        },
        async () => {
          acknowledged = true;
          return {};
        },
      ),
      /open failed/,
    );
    assert.equal(acknowledged, false);
  });
}
