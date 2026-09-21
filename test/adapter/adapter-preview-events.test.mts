import * as assert from "node:assert/strict";

import { FlexiMarkAdapter } from "../../adapters/vscode/src/adapter.mjs";
import type { DaemonOrigin } from "../../adapters/vscode/src/document-coordinator.mjs";
import type { JsonRpcConnection } from "../../adapters/vscode/src/rpc.mjs";
import type {
  PreviewState,
  WorkspaceRuntime,
} from "../../adapters/vscode/src/runtime-state.mjs";
import { deferred } from "./async-helpers.mjs";
import {
  ExtensionMode,
  Range,
  Selection,
  __test as vscode,
} from "./vscode-pure-stub.mjs";

export const suiteName = "Adapter preview event seam";

export function suite(): void {
  teardown(() => vscode.reset());

  test("rechecks every source-navigation identity after the editor await", async () => {
    const cases: [string, (state: TestState) => void][] = [
      ["membership", ({ runtime }) => void runtime.previews.clear()],
      [
        "origin",
        ({ preview }) => void (preview.origin = origin("replacement", [])),
      ],
      ["revision", ({ preview }) => void (preview.renderRevision += 1)],
      ["open document", () => void vscode.textDocuments.splice(0)],
      ["document version", ({ document }) => void (document.version += 1)],
    ];
    for (const [name, mutate] of cases) {
      const state = testState();
      const shown = deferred<TestEditor>();
      vscode.showTextDocument = () => shown.promise;
      const navigating = state.adapter.previewNavigationForTest(
        state.runtime,
        state.preview,
        sourceEvent(),
      );
      await Promise.resolve();
      mutate(state);
      shown.resolve(state.editor);
      await navigating;
      assert.equal(state.editor.selection, undefined, name);
      assert.deepEqual(state.editor.reveals, [], name);
      state.adapter.dispose();
    }

    const state = testState();
    const shown = deferred<TestEditor>();
    vscode.showTextDocument = () => shown.promise;
    const navigating = state.adapter.previewNavigationForTest(
      state.runtime,
      state.preview,
      sourceEvent(),
    );
    await Promise.resolve();
    const wrongEditor = editor(document("file:///other.md"));
    shown.resolve(wrongEditor);
    await navigating;
    assert.equal(wrongEditor.selection, undefined, "editor identity");
    assert.deepEqual(wrongEditor.reveals, [], "editor identity");
    state.adapter.dispose();
  });

  test("suppresses navigation echoes once and forwards subsequent editor events", async () => {
    const originalSetTimeout = globalThis.setTimeout;
    globalThis.setTimeout = (() => 1) as unknown as typeof setTimeout;
    const state = testState();
    vscode.visibleTextEditors = [state.editor];
    try {
      await state.adapter.previewNavigationForTest(
        state.runtime,
        state.preview,
        sourceEvent(),
      );
      assert.ok(state.editor.selection);
      assert.deepEqual(state.editor.reveals, [0]);
      assert.equal(state.notifications.length, 0);

      const selectionEvent = {
        textEditor: state.editor,
        selections: [state.editor.selection],
      } as never;
      state.adapter.selectionChanged(selectionEvent);
      assert.equal(state.notifications.length, 0);
      state.adapter.selectionChanged(selectionEvent);
      assert.equal(state.notifications[0]?.method, "fleximark/setSelection");

      const viewportEvent = {
        textEditor: state.editor,
        visibleRanges: [new Range(0, 0, 0, 1)],
      } as never;
      state.adapter.viewportChanged(viewportEvent);
      assert.equal(state.notifications.length, 1);
      state.adapter.viewportChanged(viewportEvent);
      assert.equal(state.notifications[1]?.method, "fleximark/setViewport");
    } finally {
      state.adapter.dispose();
      globalThis.setTimeout = originalSetTimeout;
    }
  });
}

interface TestDocument {
  uri: { toString(): string };
  version: number;
  lineCount: number;
  lineAt(line: number): { text: string };
}

interface TestEditor {
  document: TestDocument;
  selection?: Selection;
  reveals: number[];
  revealRange(_range: Range, kind: number): void;
}

interface TestState {
  adapter: FlexiMarkAdapter;
  document: TestDocument;
  editor: TestEditor;
  notifications: { method: string; params: unknown }[];
  preview: PreviewState;
  runtime: WorkspaceRuntime;
}

function testState(): TestState {
  vscode.reset();
  const opened = document("file:///note.md");
  const activeEditor = editor(opened);
  const notifications: { method: string; params: unknown }[] = [];
  const activeOrigin = origin("daemon", notifications);
  const workspace = { uri: { toString: () => "file:///workspace" } };
  const runtime = {
    workspace,
    documents: new Map([
      [opened.uri.toString(), { sessionId: "document", version: 1 }],
    ]),
    previews: new Map(),
    removed: false,
  } as unknown as WorkspaceRuntime;
  const preview = {
    origin: activeOrigin,
    documentUri: opened.uri.toString(),
    previewSessionId: "preview",
    target: "embeddedHtml",
    renderRevision: 1,
    notifiedRevision: 1,
  } as PreviewState;
  vscode.workspaceFolder = workspace;
  vscode.textDocuments = [opened];
  const adapter = new FlexiMarkAdapter({
    extensionMode: ExtensionMode.Test,
    extensionUri: { fsPath: "C:/extension" },
    extension: { packageJSON: { version: "test" } },
  } as never);
  return {
    adapter,
    document: opened,
    editor: activeEditor,
    notifications,
    preview,
    runtime,
  };
}

function document(uri: string): TestDocument {
  return {
    uri: { toString: () => uri },
    version: 1,
    lineCount: 1,
    lineAt: () => ({ text: "x" }),
  };
}

function editor(opened: TestDocument): TestEditor {
  return {
    document: opened,
    reveals: [],
    revealRange(_range, kind) {
      this.reveals.push(kind);
    },
  };
}

function origin(
  daemonInstanceId: string,
  notifications: { method: string; params: unknown }[],
): DaemonOrigin {
  return {
    daemonInstanceId,
    generation: 1,
    rpc: {
      closed: false,
      notify: (method: string, params: unknown) =>
        void notifications.push({ method, params }),
      request: async () => null,
    } as unknown as JsonRpcConnection,
  };
}

function sourceEvent() {
  return {
    type: "selectSource" as const,
    sourceRange: {
      byteStart: 0,
      byteEnd: 1,
      start: { line: 0, character: 0, encoding: "utf8" as const },
      end: { line: 0, character: 1, encoding: "utf8" as const },
    },
  };
}
