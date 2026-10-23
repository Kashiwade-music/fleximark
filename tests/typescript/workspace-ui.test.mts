import * as assert from "node:assert/strict";

import {
  categoryPaths,
  isCommandResult,
  isWorkspaceState,
  readResponse,
  validateFileName,
} from "../../web/workspace-ui/model.mjs";

export const suiteName = "Shared workspace workflow";
export function suite(): void {
  test("preserves selectable intermediate and nested category paths", () => {
    assert.deepEqual(
      categoryPaths([
        { name: "Work", children: [{ name: "Notes", children: [] }] },
      ]),
      [["Work"], ["Work", "Notes"]],
    );
  });
  test("rejects incomplete server context and malformed note options", () => {
    assert.equal(isWorkspaceState({ workspaces: [], documents: [] }), true);
    assert.equal(
      isWorkspaceState({
        workspaces: [{ uri: "file:///work" }],
        documents: [],
      }),
      false,
    );
    assert.equal(
      isWorkspaceState({
        workspaces: [],
        documents: [{ uri: "file:///a", version: 1.5 }],
      }),
      false,
    );
    assert.equal(
      isWorkspaceState({
        workspaces: [],
        documents: [],
        noteOptions: {
          categories: [{ name: "x", children: "bad" }],
          templates: [],
        },
      }),
      false,
    );
  });
  test("validates note names before sending a command", () => {
    for (const name of [
      "",
      " ",
      "../secret",
      "folder/note",
      ".",
      "..",
      "a".repeat(256),
    ])
      assert.equal(validateFileName(name), false, name);
    assert.equal(validateFileName("Meeting notes.md"), true);
  });
  test("surfaces server errors and rejects invalid success payloads", async () => {
    const response = (value: unknown, status = 200) =>
      ({ ok: status === 200, status, json: async () => value }) as Response;
    await assert.rejects(
      readResponse(
        response({ message: "Document changed; select it again" }, 409),
        isCommandResult,
      ),
      /Document changed/,
    );
    await assert.rejects(
      readResponse(response({ openUri: 42 }), isCommandResult),
      /invalid response/,
    );
  });
}
