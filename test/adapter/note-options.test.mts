import * as assert from "node:assert/strict";

import { executeCreateNote } from "../../adapters/vscode/src/adapter.mjs";
import type {
  ExecuteCommandParams,
  GetNoteOptionsParams,
} from "../../adapters/vscode/src/protocol.mjs";

export const suiteName = "Note option adapter";

export function suite(): void {
  test("puts the selected service options on the executeCommand wire DTO", async () => {
    const base: ExecuteCommandParams = {
      daemonInstanceId: "daemon-1",
      command: "createNote",
      workspaceUri: "file:///workspace",
    };
    let requested: GetNoteOptionsParams | undefined;
    const selections = ["work/project", "meeting"];
    let executed: ExecuteCommandParams | undefined;
    await executeCreateNote(
      base,
      async (params) => {
        requested = params;
        return {
          categories: ["work/project", "personal"],
          templates: ["meeting", "blank"],
        };
      },
      async () => selections.shift(),
      async (params) => {
        executed = params;
        return {};
      },
    );
    assert.deepEqual(requested, {
      daemonInstanceId: "daemon-1",
      workspaceUri: "file:///workspace",
    });
    assert.deepEqual(executed, {
      ...base,
      noteCategory: "work/project",
      noteTemplate: "meeting",
    });
  });

  test("does not send createNote when option selection is cancelled", async () => {
    let executed = false;
    const result = await executeCreateNote(
      {
        daemonInstanceId: "daemon-1",
        command: "createNote",
        workspaceUri: "file:///workspace",
      },
      async () => ({ categories: ["work"], templates: ["blank"] }),
      async () => undefined,
      async () => {
        executed = true;
        return {};
      },
    );
    assert.equal(result, undefined);
    assert.equal(executed, false);
  });
}
