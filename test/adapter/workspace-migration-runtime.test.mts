import * as assert from "node:assert/strict";
import * as vscode from "vscode";

import {
  detectLegacyWorkspace,
  migrateWorkspace,
} from "../../adapters/vscode/src/workspace-migration.mjs";

export const suiteName = "Legacy workspace migration runtime";

export function suite(): void {
  test("detects a legacy marker and migrates without removing legacy files", async () => {
    const parent = vscode.workspace.workspaceFolders?.[0];
    assert.ok(parent);
    const root = vscode.Uri.joinPath(
      parent.uri,
      `.migration-test-${Date.now()}-${Math.random().toString(16).slice(2)}`,
    );
    const control = vscode.Uri.joinPath(root, ".fleximark");
    const marker = vscode.Uri.joinPath(control, "fleximark.json");
    const oldTheme = vscode.Uri.joinPath(control, "fleximark.css");
    const oldPlugin = vscode.Uri.joinPath(control, "parserPlugin.js");
    const themeContents = Buffer.from("body { color: rebeccapurple; }\n");
    const workspace: vscode.WorkspaceFolder = {
      index: parent.index,
      name: "Legacy migration test",
      uri: root,
    };

    await vscode.workspace.fs.createDirectory(control);
    await vscode.workspace.fs.createDirectory(
      vscode.Uri.joinPath(root, "attachments"),
    );
    await vscode.workspace.fs.writeFile(marker, Buffer.from('{"meta":"old"}'));
    await vscode.workspace.fs.writeFile(oldTheme, themeContents);
    await vscode.workspace.fs.writeFile(
      oldPlugin,
      Buffer.from("module.exports = {};"),
    );
    try {
      const state = await detectLegacyWorkspace(workspace);
      assert.ok(state);
      assert.equal(state.hasLegacyTheme, true);
      assert.equal(state.hasLegacyPlugin, true);
      assert.equal(state.hasAttachments, true);

      assert.deepEqual(await migrateWorkspace(workspace, state), {
        legacyPluginRetained: true,
      });
      const config = Buffer.from(
        await vscode.workspace.fs.readFile(
          vscode.Uri.joinPath(control, "config.toml"),
        ),
      ).toString();
      assert.match(config, /^schema_version = 1$/m);
      assert.match(config, /roots = \["attachments"\]/);
      assert.deepEqual(
        await vscode.workspace.fs.readFile(
          vscode.Uri.joinPath(control, "theme.css"),
        ),
        themeContents,
      );
      await vscode.workspace.fs.stat(marker);
      await vscode.workspace.fs.stat(oldTheme);
      await vscode.workspace.fs.stat(oldPlugin);
      assert.equal(await detectLegacyWorkspace(workspace), undefined);
    } finally {
      await vscode.workspace.fs.delete(root, {
        recursive: true,
        useTrash: false,
      });
    }
  });
}
