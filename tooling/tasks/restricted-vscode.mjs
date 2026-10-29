import { downloadAndUnzipVSCode } from "@vscode/test-electron";
import assert from "node:assert/strict";
import { execFileSync, spawn } from "node:child_process";
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import process from "node:process";
import { fileURLToPath } from "node:url";

// test-electron's runTests() disables workspace trust. Use the official CLI's
// resolved runner/configuration, but launch official Code directly for this gate.
const root = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
const configurations = JSON.parse(
  execFileSync(
    process.execPath,
    [
      join(root, "node_modules/@vscode/test-cli/out/bin.mjs"),
      "--config",
      "adapters/vscode/.vscode-test.mjs",
      "--label",
      "standalone",
      "--list-configuration",
    ],
    { cwd: root, encoding: "utf8" },
  ),
);
assert.equal(configurations.length, 1);
const prepared = configurations[0];
assert.equal(prepared.config.label, "standalone");
assert.equal(prepared.config.workspaceFolder, undefined);
const profile = mkdtempSync(join(tmpdir(), "fleximark-restricted-vscode-"));
try {
  mkdirSync(join(profile, "User"));
  writeFileSync(
    join(profile, "User/settings.json"),
    JSON.stringify({
      "security.workspace.trust.emptyWindow": false,
      "security.workspace.trust.untrustedFiles": "open",
    }),
  );
  const executable = await downloadAndUnzipVSCode("stable");
  const args = [
    ...prepared.config.launchArgs.filter(
      (argument) => !argument.startsWith("--user-data-dir="),
    ),
    `--user-data-dir=${profile}`,
    `--extensions-dir=${join(profile, "extensions")}`,
    "--no-sandbox",
    "--disable-gpu-sandbox",
    "--disable-updates",
    "--no-cached-data",
    ...prepared.extensionDevelopmentPath.map(
      (path) => `--extensionDevelopmentPath=${path}`,
    ),
    `--extensionTestsPath=${prepared.extensionTestsPath}`,
  ];
  assert.ok(args.every((argument) => argument.startsWith("--")));
  assert.ok(!args.includes("--disable-workspace-trust"));
  const env = {
    ...process.env,
    ...prepared.env,
    FLEXIMARK_EXPECT_RESTRICTED: "1",
  };
  delete env.ELECTRON_RUN_AS_NODE;
  const code = await new Promise((resolveExit, reject) => {
    const child = spawn(executable, args, {
      cwd: root,
      env,
      stdio: "inherit",
      windowsHide: true,
    });
    child.once("error", reject);
    child.once("exit", (exitCode) => resolveExit(exitCode ?? 1));
  });
  process.exitCode = code;
} finally {
  rmSync(profile, { recursive: true, force: true, maxRetries: 3 });
}
