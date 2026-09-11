import * as assert from "node:assert/strict";

import {
  selectWorkspaceUri,
  sourcePositionToCharacter,
} from "../../adapters/vscode/src/adapter.mjs";

export const suiteName = "Multi-root workspace adapter";

export function suite(): void {
  const workspaces = [
    { label: "Alpha", uri: "file:///alpha" },
    { label: "Beta", uri: "file:///beta" },
  ];

  test("uses the active document workspace without prompting", async () => {
    let prompted = false;
    const selected = await selectWorkspaceUri(
      "file:///beta",
      workspaces,
      async () => {
        prompted = true;
        return undefined;
      },
    );
    assert.equal(selected, "file:///beta");
    assert.equal(prompted, false);
  });

  test("asks for a command workspace when no editor is active", async () => {
    const selected = await selectWorkspaceUri(
      undefined,
      workspaces,
      async (items) => items[1],
    );
    assert.equal(selected, "file:///beta");
  });

  test("converts daemon source positions to VS Code UTF-16 characters", () => {
    const line = "A😀éZ";
    assert.equal(
      sourcePositionToCharacter(line, {
        line: 0,
        character: 7,
        encoding: "utf8",
      }),
      4,
    );
    assert.equal(
      sourcePositionToCharacter(line, {
        line: 0,
        character: 3,
        encoding: "utf32",
      }),
      4,
    );
    assert.equal(
      sourcePositionToCharacter(line, {
        line: 0,
        character: 4,
        encoding: "utf16",
      }),
      4,
    );
  });
}
