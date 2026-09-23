import * as assert from "node:assert/strict";

import {
  executeCreateNote,
  selectNoteCategory,
} from "../../adapters/vscode/src/adapter.mjs";
import type {
  ExecuteCommandParams,
  GetNoteOptionsParams,
} from "../../adapters/vscode/src/protocol.mjs";

export const suiteName = "Note option adapter";

export function suite(): void {
  test("executes selected note options and stops after cancellation", async () => {
    const base: ExecuteCommandParams = {
      daemonInstanceId: "daemon-1",
      command: "createNote",
      workspaceUri: "file:///workspace",
    };
    let requested: GetNoteOptionsParams | undefined;
    let categoryStep = 0;
    let executed: ExecuteCommandParams | undefined;
    await executeCreateNote(
      base,
      async (params) => {
        requested = params;
        return {
          categories: [
            {
              id: "work",
              label: "Work",
              children: [
                { id: "work-project", label: "Project", children: [] },
              ],
            },
            { id: "personal", label: "Personal", children: [] },
          ],
          templates: ["meeting", "blank"],
        };
      },
      async (items) => {
        const id = categoryStep++ === 0 ? "work" : "work-project";
        return items.find((item) => item.category?.id === id);
      },
      async () => "meeting",
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
      noteCategoryId: "work-project",
      noteTemplate: "meeting",
    });
    let executedAfterCancellation = false;
    const result = await executeCreateNote(
      base,
      async () => ({
        categories: [{ id: "work", label: "Work", children: [] }],
        templates: ["blank"],
      }),
      async () => undefined,
      async () => "blank",
      async () => {
        executedAfterCancellation = true;
        return {};
      },
    );
    assert.equal(result, undefined);
    assert.equal(executedAfterCancellation, false);
  });

  test("supports choosing a parent and disambiguates equal labels by id", async () => {
    const categories = [
      {
        id: "left",
        label: "Same",
        children: [{ id: "left-child", label: "Same", children: [] }],
      },
      { id: "right", label: "Same", children: [] },
    ];
    let step = 0;
    const selected = await selectNoteCategory(categories, async (items) => {
      if (step++ === 0) {
        assert.deepEqual(
          items.map((item) => item.description),
          ["left", "right"],
        );
        return items.find((item) => item.category?.id === "left");
      }
      return items.find((item) => item.action === "select");
    });
    assert.equal(selected, "left");
  });
}
