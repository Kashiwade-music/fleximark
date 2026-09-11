import * as assert from "node:assert/strict";
import { createHash } from "node:crypto";
import * as vscode from "vscode";

import { FlexiMarkAdapter } from "../../adapters/vscode/src/adapter.mjs";

export const suiteName = "Daemon runtime boundary";

const deferredDisposals: FlexiMarkAdapter[] = [];

function installedExtension(): vscode.Extension<unknown> {
  const extension = vscode.extensions.getExtension("Kashiwade.fleximark");
  assert.ok(extension);
  return extension;
}

function adapterContext(extensionUri: vscode.Uri): vscode.ExtensionContext {
  return {
    extension: installedExtension(),
    extensionMode: vscode.ExtensionMode.Test,
    extensionUri,
  } as unknown as vscode.ExtensionContext;
}

function activeWorkspace(): vscode.WorkspaceFolder {
  const workspace = vscode.workspace.workspaceFolders?.[0];
  assert.ok(workspace, "integration tests must open a workspace folder");
  return workspace;
}

async function writeManifest(
  root: vscode.Uri,
  manifest: unknown,
): Promise<void> {
  const bin = vscode.Uri.joinPath(root, "bin");
  await vscode.workspace.fs.createDirectory(bin);
  await vscode.workspace.fs.writeFile(
    vscode.Uri.joinPath(bin, "manifest.json"),
    Buffer.from(`${JSON.stringify(manifest)}\n`),
  );
}

async function withTemporaryExtensionRoot(
  run: (root: vscode.Uri) => Promise<void>,
): Promise<void> {
  const workspaceFile = vscode.workspace.workspaceFile;
  assert.ok(workspaceFile?.scheme === "file");
  const root = vscode.Uri.joinPath(
    workspaceFile.with({ path: workspaceFile.path.replace(/\/[^/]*$/, "") }),
    `daemon-runtime-${Date.now()}-${Math.random().toString(16).slice(2)}`,
  );
  await vscode.workspace.fs.createDirectory(root);
  try {
    await run(root);
  } finally {
    await vscode.workspace.fs.delete(root, {
      recursive: true,
      useTrash: false,
    });
  }
}

export function suite(): void {
  suiteTeardown(() => {
    for (const adapter of deferredDisposals.reverse()) adapter.dispose();
  });

  test("rejects unsupported target, escaping path, and invalid daemon hashes", async () => {
    assert.equal(
      vscode.workspace
        .getConfiguration("fleximark")
        .get<string>("daemonPath", ""),
      "",
      "manifest tests require bundled-daemon resolution",
    );
    await withTemporaryExtensionRoot(async (root) => {
      const binary = vscode.Uri.joinPath(root, "bin", "fleximarkd-test");
      const binaryBytes = Buffer.from("not an executable");
      await vscode.workspace.fs.createDirectory(
        vscode.Uri.joinPath(root, "bin"),
      );
      await vscode.workspace.fs.writeFile(binary, binaryBytes);
      const validHash = createHash("sha256").update(binaryBytes).digest("hex");
      const cases = [
        {
          manifest: { schemaVersion: 2, protocolVersion: 1, artifacts: [] },
          message: "Unsupported FlexiMark release manifest",
        },
        {
          manifest: {
            schemaVersion: 1,
            protocolVersion: 1,
            artifacts: [
              {
                platform: "unsupported-platform",
                arch: "unsupported-arch",
                path: "bin/fleximarkd-test",
                sha256: validHash,
              },
            ],
          },
          message: `FlexiMark does not support ${process.platform}-${process.arch}`,
        },
        {
          manifest: {
            schemaVersion: 1,
            protocolVersion: 1,
            artifacts: [
              {
                platform: process.platform,
                arch: process.arch,
                path: "../outside-fleximarkd",
                sha256: validHash,
              },
            ],
          },
          message: "FlexiMark release manifest path escapes the extension",
        },
        {
          manifest: {
            schemaVersion: 1,
            protocolVersion: 1,
            artifacts: [
              {
                platform: process.platform,
                arch: process.arch,
                path: "bin/fleximarkd-test",
                sha256: "INVALID",
              },
            ],
          },
          message: "FlexiMark release manifest has an invalid checksum",
        },
        {
          manifest: {
            schemaVersion: 1,
            protocolVersion: 1,
            artifacts: [
              {
                platform: process.platform,
                arch: process.arch,
                path: "bin/fleximarkd-test",
                sha256: "0".repeat(64),
              },
            ],
          },
          message:
            "Bundled FlexiMark daemon is corrupt or does not match this extension",
        },
      ];

      const adapter = new FlexiMarkAdapter(adapterContext(root));
      deferredDisposals.push(adapter);
      for (const { manifest, message } of cases) {
        await writeManifest(root, manifest);
        await assert.rejects(adapter.start(activeWorkspace()), { message });
        assert.deepEqual(adapter.recoveryStateForTest(), {
          connectionGeneration: 0,
          daemonInstanceId: undefined,
          documentSessions: {},
          previewSessions: {},
          previewRenderRevisions: {},
        });
      }
    });
  });

  test("clears a failed spawn and permits a fresh launch attempt", async () => {
    await withTemporaryExtensionRoot(async (root) => {
      const binaryBytes = Buffer.from("not an executable");
      await vscode.workspace.fs.createDirectory(
        vscode.Uri.joinPath(root, "bin"),
      );
      await vscode.workspace.fs.writeFile(
        vscode.Uri.joinPath(root, "bin", "fleximarkd-test"),
        binaryBytes,
      );
      await writeManifest(root, {
        schemaVersion: 1,
        protocolVersion: 1,
        artifacts: [
          {
            platform: process.platform,
            arch: process.arch,
            path: "bin/fleximarkd-test",
            sha256: createHash("sha256").update(binaryBytes).digest("hex"),
          },
        ],
      });
      const adapter = new FlexiMarkAdapter(adapterContext(root));
      deferredDisposals.push(adapter);
      const concurrent = await Promise.allSettled([
        adapter.start(activeWorkspace()),
        adapter.start(activeWorkspace()),
      ]);
      assert.deepEqual(
        concurrent.map(({ status }) => status),
        ["rejected", "rejected"],
      );
      assert.deepEqual(adapter.recoveryStateForTest(), {
        connectionGeneration: 1,
        daemonInstanceId: undefined,
        documentSessions: {},
        previewSessions: {},
        previewRenderRevisions: {},
      });

      await assert.rejects(adapter.start(activeWorkspace()));
      assert.equal(adapter.recoveryStateForTest().connectionGeneration, 2);
    });
  });

  test("coalesces concurrent starts and synchronously resets state on dispose", async () => {
    const extension = installedExtension();
    const adapter = new FlexiMarkAdapter(
      adapterContext(extension.extensionUri),
    );
    try {
      const [first, second] = await Promise.all([
        adapter.start(activeWorkspace()),
        adapter.start(activeWorkspace()),
      ]);
      assert.equal(first, second);
      const running = adapter.recoveryStateForTest();
      assert.equal(running.connectionGeneration, 1);
      assert.ok(running.daemonInstanceId);
    } finally {
      adapter.dispose();
    }
    assert.deepEqual(adapter.recoveryStateForTest(), {
      connectionGeneration: 1,
      daemonInstanceId: undefined,
      documentSessions: {},
      previewSessions: {},
      previewRenderRevisions: {},
    });
    await adapter.start(activeWorkspace());
    await adapter.activateDocument(vscode.window.activeTextEditor?.document);
    assert.deepEqual(adapter.recoveryStateForTest(), {
      connectionGeneration: 1,
      daemonInstanceId: undefined,
      documentSessions: {},
      previewSessions: {},
      previewRenderRevisions: {},
    });
  });
}
