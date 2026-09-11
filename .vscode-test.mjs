import { defineConfig } from "@vscode/test-cli";
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import process from "node:process";

const testUserDataDirectory = mkdtempSync(
  join(tmpdir(), "fleximark-vscode-test-"),
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

export default defineConfig({
  files: "out/test/**/*.test.cjs",
  launchArgs: [`--user-data-dir=${testUserDataDirectory}`],
  workspaceFolder: integrationWorkspaceFile,
});
