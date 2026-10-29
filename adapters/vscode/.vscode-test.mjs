import { defineConfig } from "@vscode/test-cli";
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { arch, platform } from "node:os";
import { join } from "node:path";
import process from "node:process";

import { RUNTIME_TARGETS as targets } from "../../contracts/distribution/distribution.generated.mjs";
import { releaseMetadata } from "../../tooling/tasks/release-metadata.mjs";

const { version } = releaseMetadata();
const testUserDataDirectory = mkdtempSync(
  join(tmpdir(), "fleximark-vscode-test-"),
);
const standaloneUserDataDirectory = join(
  testUserDataDirectory,
  "standalone-profile",
);
mkdirSync(join(standaloneUserDataDirectory, "User"), { recursive: true });
writeFileSync(
  join(standaloneUserDataDirectory, "User", "settings.json"),
  JSON.stringify({
    "security.workspace.trust.emptyWindow": false,
    "security.workspace.trust.untrustedFiles": "open",
  }),
);
process.once("exit", () => {
  try {
    rmSync(testUserDataDirectory, {
      force: true,
      maxRetries: 3,
      recursive: true,
    });
  } catch {
    // Test exit status is authoritative; stale temporary data is best-effort cleanup.
  }
});
const stableWorkspaceDirectory = join(testUserDataDirectory, "workspace");
const integrationWorkspaceFile = join(
  testUserDataDirectory,
  "integration.code-workspace",
);
mkdirSync(stableWorkspaceDirectory);
writeFileSync(
  integrationWorkspaceFile,
  `${JSON.stringify({ folders: [{ path: "workspace" }] }, undefined, 2)}\n`,
);

const extensionDevelopmentPath = `../../.artifacts/${version}/${targets[`${platform()}-${arch()}`]}/vscode`;

export default defineConfig([
  {
    label: "workspace",
    extensionDevelopmentPath,
    files: "../../.build/tests/electron/extension.test.cjs",
    launchArgs: [`--user-data-dir=${testUserDataDirectory}`],
    workspaceFolder: integrationWorkspaceFile,
  },
  {
    label: "standalone",
    extensionDevelopmentPath,
    files: "../../.build/tests/electron/standalone.test.cjs",
    launchArgs: [
      `--user-data-dir=${standaloneUserDataDirectory}`,
      "--new-window",
      "--skip-welcome",
      "--skip-release-notes",
    ],
  },
]);
