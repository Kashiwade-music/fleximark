import * as assert from "node:assert/strict";

import {
  type LegacyWorkspaceState,
  WorkspaceMigrationController,
  createMigratedConfig,
} from "../../adapters/vscode/src/workspace-migration-policy.mjs";

export const suiteName = "Legacy workspace migration";

const legacyState: LegacyWorkspaceState = {
  hasLegacyTheme: true,
  hasLegacyPlugin: false,
  hasAttachments: true,
  settings: {},
};

export function suite(): void {
  test("converts legacy note settings into canonical version 1 TOML", () => {
    const config = createMigratedConfig(
      {
        noteCategories: {
          General: { Daily: {}, Reports: { Weekly: {} } },
          Unsafe: { "..": {} },
          "a/b": {},
        },
        noteFileNamePrefix: "${CURRENT_YEAR}_",
        noteFileNameSuffix: "_draft",
        noteTemplates: {
          default: ["# ${1:Title}", "Created ${CURRENT_DATE}"],
          invalid: "not an array",
        },
      },
      true,
    );

    assert.match(config, /^schema_version = 1$/m);
    assert.match(config, /file_name_prefix = "\$\{CURRENT_YEAR\}_"/);
    assert.match(config, /"General" = "General"/);
    assert.match(
      config,
      /"General \/ Reports \/ Weekly" = "General\/Reports\/Weekly"/,
    );
    assert.doesNotMatch(config, /\.\./);
    assert.doesNotMatch(config, /a\/b/);
    assert.match(
      config,
      /"default" = \["# \$\{1:Title\}", "Created \$\{CURRENT_DATE\}"\]/,
    );
    assert.doesNotMatch(config, /invalid/);
    assert.match(config, /roots = \["attachments"\]/);
    assert.match(config, /raw_html_export = "reject"/);
  });

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
        return { legacyPluginRetained: false };
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
