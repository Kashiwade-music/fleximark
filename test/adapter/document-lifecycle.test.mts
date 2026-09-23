import * as assert from "node:assert/strict";
import * as vscode from "vscode";

import { FlexiMarkAdapter } from "../../adapters/vscode/src/adapter.mjs";
import { handleRequestFullText } from "../../adapters/vscode/src/document-coordinator.mjs";
import type { JsonRpcConnection } from "../../adapters/vscode/src/rpc.mjs";
import type { WorkspaceRuntime } from "../../adapters/vscode/src/runtime-state.mjs";

export const suiteName = "Document lifecycle adapter";

function fullTextRequest(
  id: number,
  daemonInstanceId: string,
  documentSessionId: string,
  uri: string,
): Parameters<typeof handleRequestFullText>[0] {
  return {
    jsonrpc: "2.0",
    id,
    method: "fleximark/requestFullText",
    params: { daemonInstanceId, documentSessionId, uri },
  };
}

export function suite(): void {
  test("answers current full-text requests after the full change notification", async () => {
    const workspace = vscode.workspace.workspaceFolders?.[0];
    assert.ok(workspace);
    const documentUri = vscode.Uri.joinPath(
      workspace.uri,
      "full-text-contract.md",
    );
    await vscode.workspace.fs.writeFile(
      documentUri,
      Buffer.from("# Current unsaved text\n"),
    );
    const document = await vscode.workspace.openTextDocument(documentUri);
    await vscode.window.showTextDocument(document);
    const uri = document.uri.toString();
    try {
      const runtime = {
        documents: new Map([
          [uri, { sessionId: "session-current", version: document.version }],
          ["file:///closed.md", { sessionId: "session-closed", version: 1 }],
        ]),
      } as unknown as WorkspaceRuntime;
      const events: unknown[] = [];
      const connection = {
        notifyLsp(method: string, params: unknown) {
          events.push({ method, params });
        },
        respond(id: number | string, result: unknown) {
          events.push({ id, result });
        },
      } as unknown as JsonRpcConnection;

      assert.equal(
        handleRequestFullText(
          fullTextRequest(1, "daemon-current", "session-current", uri),
          connection,
          [runtime],
          "daemon-current",
          [document],
        ),
        true,
      );
      assert.deepEqual(events, [
        {
          method: "textDocument/didChange",
          params: {
            textDocument: { uri, version: document.version },
            contentChanges: [{ text: "# Current unsaved text\n" }],
          },
        },
        { id: 1, result: null },
      ]);

      events.length = 0;
      handleRequestFullText(
        fullTextRequest(
          2,
          "daemon-current",
          "session-closed",
          "file:///closed.md",
        ),
        connection,
        [runtime],
        "daemon-current",
        [],
      );
      assert.deepEqual(events, [{ id: 2, result: null }]);

      events.length = 0;
      handleRequestFullText(
        fullTextRequest(3, "daemon-stale", "session-current", uri),
        connection,
        [runtime],
        "daemon-current",
        [document],
      );
      assert.deepEqual(events, []);
    } finally {
      await closeTextTab(uri);
      await vscode.workspace.fs.delete(documentUri, { useTrash: false });
    }
  });

  test("publishes diagnostics from the daemon", async function () {
    this.timeout(15_000);
    const extension = vscode.extensions.getExtension("Kashiwade.fleximark");
    const workspace = vscode.workspace.workspaceFolders?.[0];
    assert.ok(extension);
    assert.ok(workspace);
    const uri = vscode.Uri.joinPath(workspace.uri, "checkpoint-contract.md");
    await vscode.workspace.fs.writeFile(
      uri,
      Buffer.from("# Heading\n\n<script>alert(1)</script>\n"),
    );
    const document = await vscode.workspace.openTextDocument(uri);
    const context = {
      extensionUri: extension.extensionUri,
      extension: { packageJSON: extension.packageJSON },
      extensionMode: vscode.ExtensionMode.Test,
    } as unknown as vscode.ExtensionContext;
    const adapter = new FlexiMarkAdapter(context);
    try {
      await adapter.start(workspace);
      await adapter.syncDocument(document);
      const diagnostics = vscode.languages.getDiagnostics(uri);
      assert.ok(diagnostics.length >= 1);
      for (const diagnostic of diagnostics)
        assert.deepEqual(
          {
            code: diagnostic.code,
            data: (diagnostic as vscode.Diagnostic & { data?: unknown }).data,
            message: diagnostic.message,
            range: [
              diagnostic.range.start.line,
              diagnostic.range.start.character,
              diagnostic.range.end.line,
              diagnostic.range.end.character,
            ],
            severity: diagnostic.severity,
            source: diagnostic.source,
          },
          {
            code: "raw-html",
            data: {
              escapedText: "&lt;script&gt;alert(1)&lt;/script&gt;\n",
            },
            message:
              "Unsafe raw HTML content was removed or rewritten in the rendered output",
            range: [2, 0, 2, 25],
            severity: vscode.DiagnosticSeverity.Warning,
            source: "fleximark",
          },
        );
    } finally {
      adapter.dispose();
      await closeTextTab(uri.toString());
      await vscode.workspace.fs.delete(uri, { useTrash: false });
    }
  });
}

async function closeTextTab(uri: string): Promise<void> {
  const tab = vscode.window.tabGroups.all
    .flatMap((group) => group.tabs)
    .find(
      (candidate) =>
        candidate.input instanceof vscode.TabInputText &&
        candidate.input.uri.toString() === uri,
    );
  if (tab) await vscode.window.tabGroups.close(tab, true);
}
