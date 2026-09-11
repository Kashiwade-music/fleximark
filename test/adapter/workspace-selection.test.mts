import * as assert from "node:assert/strict";

import {
  findVisibleSourceEditor,
  previewEventAction,
  selectWorkspaceUri,
  sourcePositionToCharacter,
  sourcePositionWithinLine,
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

  test("rejects source characters outside their contextual encoding bounds", () => {
    const line = "A😀éZ";
    assert.equal(
      sourcePositionWithinLine(line, {
        line: 0,
        character: Buffer.byteLength(line, "utf8"),
        encoding: "utf8",
      }),
      true,
    );
    assert.equal(
      sourcePositionWithinLine(line, {
        line: 0,
        character: Buffer.byteLength(line, "utf8") + 1,
        encoding: "utf8",
      }),
      false,
    );
    assert.equal(
      sourcePositionWithinLine(line, {
        line: 0,
        character: [...line].length + 1,
        encoding: "utf32",
      }),
      false,
    );
  });

  test("classifies stale preview events without changing adapter state", () => {
    const base = {
      daemonInstanceId: "daemon",
      previewSessionId: "preview",
      renderRevision: 4,
    };
    assert.equal(
      previewEventAction(4, {
        ...base,
        event: {
          type: "full",
          previewSessionId: "preview",
          documentVersion: 1,
          resultRenderRevision: 4,
          rendererFingerprint: "sha256:renderer",
          nodeIds: ["document-root"],
          navigation: [],
          style: null,
          assets: [],
          html: '<main data-fleximark-node-id="document-root"></main>',
        },
      }),
      "ignore",
    );
    assert.equal(
      previewEventAction(4, {
        ...base,
        renderRevision: 6,
        event: {
          type: "patch",
          previewSessionId: "preview",
          documentVersion: 2,
          baseRenderRevision: 5,
          resultRenderRevision: 6,
          baseRendererFingerprint: "sha256:renderer",
          resultRendererFingerprint: "sha256:renderer",
          navigation: [],
          style: null,
          operations: [],
        },
      }),
      "reload",
    );
    assert.equal(
      previewEventAction(4, {
        ...base,
        renderRevision: 3,
        event: {
          type: "viewport",
          previewSessionId: "preview",
          renderRevision: 3,
          nodeId: "a",
        },
      }),
      "ignore",
    );
  });

  test("reuses the source editor in its original column", () => {
    const sourceUri = "file:///notes/example.md";
    const otherColumn = {
      document: { uri: { toString: () => sourceUri } },
      viewColumn: 2,
    };
    const originalColumn = {
      document: { uri: { toString: () => sourceUri } },
      viewColumn: 1,
    };

    assert.equal(
      findVisibleSourceEditor([otherColumn, originalColumn], sourceUri, 1),
      originalColumn,
    );
  });

  test("does not reuse an editor for a different document", () => {
    const editor = {
      document: { uri: { toString: () => "file:///notes/other.md" } },
      viewColumn: 1,
    };

    assert.equal(
      findVisibleSourceEditor([editor], "file:///notes/example.md", 1),
      undefined,
    );
  });
}
