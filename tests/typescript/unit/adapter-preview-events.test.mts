import { WORKSPACE_COMMAND_KIND } from "@fleximark/contracts/protocol";
import { FlexiMarkAdapter } from "fleximark/src/adapter";
import { executeMigration } from "fleximark/src/commands";
import { DaemonConnection } from "fleximark/src/daemon-connection";
import { EditorEventForwarder } from "fleximark/src/editor-event-forwarder";
import type { JsonRpcConnection } from "fleximark/src/rpc";
import {
  EmbeddedBinding,
  type EmbeddedPreview,
} from "fleximark/src/runtime-state";
import type {
  DocumentRuntime,
  PreviewState,
} from "fleximark/src/runtime-state";
import { isShowDocumentParams } from "fleximark/src/show-document";
import { applySourceNavigation } from "fleximark/src/source-navigation";
import * as assert from "node:assert/strict";

import { deferred } from "../support/async-helpers.mjs";
import { renderFrame } from "../support/preview-harness.mjs";
import {
  ExtensionMode,
  Range,
  Selection,
  __test as vscode,
  workspace as workspaceApi,
} from "../support/vscode-pure-stub.mjs";

export const suiteName = "Adapter preview event seam";

export function suite(): void {
  test("ignores non-Markdown active buffers before starting document services", async () => {
    const state = testState();
    let starts = 0;
    state.adapter.start = async () => {
      starts += 1;
      return undefined;
    };
    const nonMarkdown = {
      ...state.document,
      languageId: "plaintext",
    } as unknown as Parameters<FlexiMarkAdapter["activateDocument"]>[0];
    try {
      await state.adapter.activateDocument(nonMarkdown);
      await state.adapter.activateEditor({ document: nonMarkdown } as never);
      assert.equal(
        await state.adapter.requestLanguageFeature(
          "textDocument/hover",
          nonMarkdown as never,
        ),
        undefined,
      );
      assert.equal(starts, 0);
    } finally {
      state.adapter.dispose();
    }
  });
  test("propagates daemon migration denial in a restricted window without applying", async () => {
    const trusted = workspaceApi.isTrusted;
    workspaceApi.isTrusted = false;
    let confirmed = false;
    const commands: string[] = [];
    try {
      await assert.rejects(
        executeMigration(
          {
            daemonInstanceId: "daemon",
            context: { workspaceUri: "file:///workspace" },
            operation: {
              command: WORKSPACE_COMMAND_KIND.planMigration,
              recover: false,
            },
          },
          async (request) => {
            commands.push(request.operation.command);
            throw new Error("Workspace is not trusted");
          },
          async () => {
            confirmed = true;
            return true;
          },
          () => true,
        ),
        /Workspace is not trusted/,
      );
      assert.deepEqual(commands, [WORKSPACE_COMMAND_KIND.planMigration]);
      assert.equal(confirmed, false);
    } finally {
      workspaceApi.isTrusted = trusted;
    }
  });
  test("validates browser URLs and source selections before opening them", () => {
    assert.equal(
      isShowDocumentParams({
        uri: "http://127.0.0.1/workspace/token",
        external: true,
        takeFocus: false,
      }),
      true,
    );
    assert.equal(
      isShowDocumentParams({
        uri: "file:///note.md",
        external: false,
        takeFocus: true,
        selection: {
          start: { line: 1, character: 2 },
          end: { line: 1, character: 3 },
        },
      }),
      true,
    );
    for (const value of [
      null,
      { uri: 4 },
      { uri: "x", takeFocus: "yes" },
      {
        uri: "x",
        selection: {
          start: { line: -1, character: 0 },
          end: { line: 0, character: 0 },
        },
      },
    ])
      assert.equal(
        isShowDocumentParams(
          value === null
            ? null
            : { external: false, takeFocus: false, ...value },
        ),
        false,
      );
  });
  teardown(() => vscode.reset());

  test("rechecks every source-navigation identity after the editor await", async () => {
    const cases: [string, (state: TestState) => void][] = [
      ["membership", ({ runtime }) => void runtime.previews.clear()],
      [
        "origin",
        ({ preview }) =>
          void (preview.binding = new EmbeddedBinding(
            origin("replacement", []),
            renderFrame(),
            () => true,
          )),
      ],
      ["revision", ({ preview }) => void (preview.binding.renderRevision += 1)],
      ["open document", () => void vscode.textDocuments.splice(0)],
      ["document version", ({ document }) => void (document.version += 1)],
    ];
    for (const [name, mutate] of cases) {
      const state = testState();
      const shown = deferred<TestEditor>();
      vscode.showTextDocument = () => shown.promise;
      const navigating = state.navigate(
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
    const navigating = state.navigate(
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
      await state.navigate(state.runtime, state.preview, sourceEvent());
      assert.ok(state.editor.selection);
      assert.deepEqual(state.editor.reveals, [0]);
      assert.equal(state.notifications.length, 0);

      const selectionEvent = {
        textEditor: state.editor,
        selections: [state.editor.selection],
      } as never;
      state.forwarder.selectionChanged(selectionEvent);
      assert.equal(state.notifications.length, 0);
      state.forwarder.selectionChanged(selectionEvent);
      assert.equal(state.notifications[0]?.method, "fleximark/setSelection");
      assert.deepEqual(state.notifications[0]?.params, {
        daemonInstanceId: "daemon",
        documentSessionId: "document",
        expectedDocumentVersion: 1,
        selections: [
          {
            anchor: { line: 0, character: 0 },
            active: { line: 0, character: 1 },
          },
        ],
      });

      const viewportEvent = {
        textEditor: state.editor,
        visibleRanges: [new Range(0, 0, 0, 1)],
      } as never;
      state.forwarder.viewportChanged(viewportEvent);
      assert.equal(state.notifications.length, 1);
      state.forwarder.viewportChanged(viewportEvent);
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
  preview: EmbeddedPreview;
  navigate: (
    runtime: DocumentRuntime,
    preview: PreviewState,
    event: Parameters<typeof applySourceNavigation>[1],
  ) => Promise<void>;
  forwarder: EditorEventForwarder;
  runtime: DocumentRuntime;
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
  } as unknown as DocumentRuntime;
  const binding: EmbeddedBinding = new EmbeddedBinding(
    activeOrigin,
    renderFrame(),
    () => runtime.previews.get("preview")?.binding === binding,
  );
  binding.renderRevision = 1;
  const preview: EmbeddedPreview = {
    binding,
    documentUri: opened.uri.toString(),
    target: "embeddedHtml",
  };
  runtime.previews.set("preview", preview);
  const echoes = {};
  const forwarder = new EditorEventForwarder(
    {
      connection: activeOrigin,
      runtimeForDocument: () => runtime,
      activateDocument: async () => undefined,
      changeDocument: () => undefined,
      closeDocument: () => undefined,
    },
    { echoes, follow: async () => undefined },
  );
  vscode.workspaceFolder = workspace;
  vscode.textDocuments = [opened];
  const adapter = new FlexiMarkAdapter({
    extensionMode: ExtensionMode.Test,
    extensionUri: { fsPath: "C:/extension" },
    extension: { packageJSON: { version: "test" } },
  } as never);
  return {
    adapter,
    forwarder,
    navigate: (_owner, current, event) =>
      applySourceNavigation(current, event, echoes),
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
): DaemonConnection {
  const rpc = {
    closed: false,
    once: () => undefined,
    close: () => undefined,
    notify: (method: string, params: unknown) =>
      void notifications.push({ method, params }),
    request: async () => null,
  } as unknown as JsonRpcConnection;
  return new DaemonConnection(rpc, 1, daemonInstanceId);
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
