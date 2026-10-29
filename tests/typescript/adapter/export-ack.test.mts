import * as assert from "node:assert/strict";

import { openCommandResult } from "../../../adapters/vscode/src/adapter.mjs";
import { WORKSPACE_COMMAND_KIND } from "../../../contracts/typescript/protocol.mjs";
import type { WorkspaceCommandRequest } from "../../../contracts/typescript/protocol.mjs";

export const suiteName = "Export acknowledgement";

export function suite(): void {
  const params: WorkspaceCommandRequest = {
    daemonInstanceId: "daemon",
    context: {
      workspaceUri: "file:///workspace",
      document: { documentSessionId: "document", expectedDocumentVersion: 3 },
    },
    operation: { command: WORKSPACE_COMMAND_KIND.exportHtml },
  };

  test("acknowledges if and only if the exported URI opens", async () => {
    const calls: string[] = [];
    await openCommandResult(
      params,
      {
        kind: "htmlExported",
        generation: 1,
        outputUri: "file:///workspace/public/index.html",
      },
      async () => {
        calls.push("open");
      },
      async (request) => {
        calls.push(request.operation.command);
        assert.deepEqual(request.operation, {
          command: WORKSPACE_COMMAND_KIND.acknowledgeExport,
          destinationUri: "file:///workspace/public/index.html",
        });
        return { kind: "exportAcknowledged" };
      },
    );
    assert.deepEqual(calls, ["open", WORKSPACE_COMMAND_KIND.acknowledgeExport]);
    let acknowledged = false;
    await assert.rejects(
      openCommandResult(
        params,
        {
          kind: "htmlExported",
          generation: 1,
          outputUri: "file:///workspace/public/index.html",
        },
        async () => {
          throw new Error("open failed");
        },
        async () => {
          acknowledged = true;
          return { kind: "exportAcknowledged" };
        },
      ),
      /open failed/,
    );
    assert.equal(acknowledged, false);
  });
}
