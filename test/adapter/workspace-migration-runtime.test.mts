import * as assert from "node:assert/strict";
import * as vscode from "vscode";

import {
  type ExecuteMigrationCommand,
  detectLegacyWorkspace,
  migrateWorkspace,
} from "../../adapters/vscode/src/workspace-migration.mjs";

export const suiteName = "Legacy workspace migration runtime";

export function suite(): void {
  test("accepts known daemon inspection results and rejects unknown ones", async () => {
    const workspace = vscode.workspace.workspaceFolders?.[0];
    assert.ok(workspace);
    const calls: { command: string; args?: readonly string[] }[] = [];
    const execute: ExecuteMigrationCommand = async (
      _workspace,
      command,
      args,
    ) => {
      calls.push({ command, args });
      return command === "inspectLegacyWorkspace"
        ? { data: "legacy-plugin" }
        : {};
    };

    const state = await detectLegacyWorkspace(workspace, execute);
    assert.ok(state);
    assert.equal(state.hasLegacyPlugin, true);
    await migrateWorkspace(workspace, state, execute);

    assert.equal(calls[0]?.command, "inspectLegacyWorkspace");
    assert.equal(calls[1]?.command, "migrateWorkspace");
    const [settings] = calls[1]?.args ?? [];
    assert.deepEqual(
      JSON.parse(settings ?? ""),
      JSON.parse(JSON.stringify(state.settings)),
    );
    await assert.rejects(
      detectLegacyWorkspace(workspace, async () => ({ data: "unknown" })),
      { message: "Invalid legacy workspace inspection result" },
    );
  });
}
