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
              name: "Work",
              children: [{ name: "Project", children: [] }],
            },
            { name: "Personal", children: [] },
          ],
          templates: ["meeting", "blank"],
        };
      },
      async (items) => {
        const name = categoryStep++ === 0 ? "Work" : "Project";
        return items.find((item) => item.category?.name === name);
      },
      async () => "meeting",
      async (options) => {
        assert.equal(options.validateInput(""), "File name cannot be empty");
        assert.equal(options.validateInput("session"), undefined);
        return "session";
      },
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
      noteCategoryPath: ["Work", "Project"],
      noteTemplate: "meeting",
      noteFileName: "session",
    });
    let executedAfterCancellation = false;
    const result = await executeCreateNote(
      base,
      async () => ({
        categories: [{ name: "Work", children: [] }],
        templates: ["blank"],
      }),
      async () => undefined,
      async () => "blank",
      async () => "cancelled-before-file-name",
      async () => {
        executedAfterCancellation = true;
        return {};
      },
    );
    assert.equal(result, undefined);
    assert.equal(executedAfterCancellation, false);

    const fileNameCancellation = await executeCreateNote(
      { ...base, command: "createNote" },
      async () => ({ categories: [], templates: [] }),
      async () => undefined,
      async () => undefined,
      async () => undefined,
      async () => {
        throw new Error("cancelled file name must not execute");
      },
    );
    assert.equal(fileNameCancellation, undefined);
  });

  test("supports choosing a parent and returns breadcrumb paths", async () => {
    const categories = [
      {
        name: "Left",
        children: [{ name: "Same", children: [] }],
      },
      { name: "Right", children: [{ name: "Same", children: [] }] },
    ];
    let step = 0;
    const selected = await selectNoteCategory(categories, async (items) => {
      if (step++ === 0) {
        assert.deepEqual(
          items.map((item) => item.description),
          ["Left", "Right"],
        );
        return items.find((item) => item.category?.name === "Left");
      }
      assert.deepEqual(
        items.map((item) => item.description),
        ["Left", "Left / Same", undefined],
      );
      return items.find((item) => item.action === "useCurrent");
    });
    assert.deepEqual(selected, ["Left"]);

    let rightStep = 0;
    const duplicateName = await selectNoteCategory(
      categories,
      async (items) => {
        const name = rightStep++ === 0 ? "Right" : "Same";
        return items.find(
          (item) =>
            item.category?.name === name && item.action !== "useCurrent",
        );
      },
    );
    assert.deepEqual(duplicateName, ["Right", "Same"]);
  });
}
