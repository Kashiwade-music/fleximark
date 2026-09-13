import * as assert from "node:assert/strict";

import {
  findVisibleSourceEditor,
  previewEventAction,
  selectWorkspaceUri,
  sourcePositionToCharacter,
  sourcePositionWithinLine,
} from "../../adapters/vscode/src/adapter.mjs";
import type { PreviewEvent } from "../../adapters/vscode/src/protocol.mjs";

export const suiteName = "Multi-root workspace adapter";

function previewMessage(
  renderRevision: number,
  event: PreviewEvent["event"],
): PreviewEvent {
  return {
    daemonInstanceId: "daemon",
    previewSessionId: "preview",
    renderRevision,
    event,
  };
}

function fullPreview(
  documentVersion: number,
  resultRenderRevision: number,
): PreviewEvent["event"] {
  return {
    type: "full",
    previewSessionId: "preview",
    documentVersion,
    resultRenderRevision,
    rendererFingerprint: "sha256:renderer",
    nodeIds: ["document-root"],
    navigation: [],
    style: null,
    assets: [],
    html: '<main data-fleximark-node-id="document-root"></main>',
  };
}

function patchPreview(
  baseRenderRevision: number,
  resultRenderRevision: number,
): PreviewEvent["event"] {
  return {
    type: "patch",
    previewSessionId: "preview",
    documentVersion: 2,
    baseRenderRevision,
    resultRenderRevision,
    baseRendererFingerprint: "sha256:renderer",
    resultRendererFingerprint: "sha256:renderer",
    navigation: [],
    style: null,
    operations: [],
  };
}

function viewportPreview(renderRevision: number): PreviewEvent["event"] {
  return {
    type: "viewport",
    previewSessionId: "preview",
    renderRevision,
    nodeId: "a",
  };
}

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

  test("classifies current, stale, and discontinuous preview revisions", () => {
    assert.equal(
      previewEventAction(4, previewMessage(4, fullPreview(1, 4))),
      "ignore",
    );
    assert.equal(
      previewEventAction(4, previewMessage(4, fullPreview(2, 5))),
      "apply",
    );
    assert.equal(
      previewEventAction(4, previewMessage(5, patchPreview(4, 5))),
      "apply",
    );
    assert.equal(
      previewEventAction(4, previewMessage(6, patchPreview(5, 6))),
      "reload",
    );
    assert.equal(
      previewEventAction(4, previewMessage(3, viewportPreview(3))),
      "ignore",
    );
    assert.equal(
      previewEventAction(4, previewMessage(4, viewportPreview(4))),
      "apply",
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
