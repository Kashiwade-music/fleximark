import * as assert from "node:assert/strict";

import {
  type LegacyWorkspaceState,
  WorkspaceMigrationController,
} from "../../adapters/vscode/src/workspace-migration-policy.mjs";

export const suiteName = "Legacy workspace migration";

const legacyState: LegacyWorkspaceState = {
  hasLegacyPlugin: false,
  settings: {},
};

export function suite(): void {
  test("does not remember cancellation across workspace reopen", async () => {
    const confirmations: string[] = [];
    const migrations: string[] = [];
    const controller = new WorkspaceMigrationController<string>({
      key: (workspace) => workspace,
      detect: async () => legacyState,
      confirm: async (workspace) => {
        confirmations.push(workspace);
        return confirmations.length > 1;
      },
      migrate: async (workspace) => {
        migrations.push(workspace);
      },
      completed: async () => undefined,
    });

    await controller.offer("workspace");
    await controller.offer("workspace");
    assert.deepEqual(confirmations, ["workspace"]);
    assert.deepEqual(migrations, []);

    controller.forget("workspace");
    await controller.offer("workspace");
    assert.deepEqual(confirmations, ["workspace", "workspace"]);
    assert.deepEqual(migrations, ["workspace"]);
  });
}
