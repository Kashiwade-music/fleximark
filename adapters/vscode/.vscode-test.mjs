import { defineConfig } from "@vscode/test-cli";
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { readFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { arch, platform } from "node:os";
import { join } from "node:path";
import process from "node:process";
import { URL } from "node:url";

const version = /^version = "([^"]+)"/m.exec(
  readFileSync(new URL("../../release.toml", import.meta.url), "utf8"),
)[1];
const targets = {
  "linux-x64": "x86_64-unknown-linux-gnu",
  "linux-arm64": "aarch64-unknown-linux-gnu",
  "darwin-x64": "x86_64-apple-darwin",
  "darwin-arm64": "aarch64-apple-darwin",
  "win32-x64": "x86_64-pc-windows-msvc",
  "win32-arm64": "aarch64-pc-windows-msvc",
};

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
