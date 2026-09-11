import * as assert from "node:assert/strict";
import * as fs from "node:fs";
import * as path from "node:path";
import * as vscode from "vscode";

export const suiteName = "Release assembly";

export function suite(): void {
  const extension = vscode.extensions.getExtension("Kashiwade.fleximark");
  assert.ok(extension);

  test("declares Windows macOS and Linux x64 and arm64 daemon artifacts", () => {
    const source = fs.readFileSync(
      path.join(extension.extensionPath, "scripts/create-release-manifest.mjs"),
      "utf8",
    );
    for (const target of ["win32", "darwin", "linux"])
      for (const arch of ["x64", "arm64"])
        assert.match(source, new RegExp('\\["' + target + '", "' + arch + '"'));
  });

  test("assembles daemons before semantic release packaging", () => {
    const workflow = fs.readFileSync(
      path.join(extension.extensionPath, ".github/workflows/release.yml"),
      "utf8",
    );
    assert.match(workflow, /pattern: daemon-\*/);
    assert.match(workflow, /create-release-manifest\.mjs --require-all/);
    assert.match(workflow, /node esbuild\.js --production/);
  });

  test("builds the external preview client before compiling the daemon", () => {
    const packageJson = JSON.parse(
      fs.readFileSync(
        path.join(extension.extensionPath, "package.json"),
        "utf8",
      ),
    );
    assert.equal(packageJson.scripts.build, "node scripts/tasks.mjs build");
    const tasks = fs.readFileSync(
      path.join(extension.extensionPath, "scripts/tasks.mjs"),
      "utf8",
    );
    const preview = tasks.indexOf(
      'runNode("scripts/build-browser-client.mjs")',
    );
    const daemon = tasks.indexOf('runCargo("build", "--release"');
    const stage = tasks.indexOf('runNode("scripts/stage-daemon.mjs")');
    const manifest = tasks.indexOf(
      'runNode("scripts/create-release-manifest.mjs")',
    );
    const adapter = tasks.indexOf('runNode("esbuild.js", "--production")');
    assert.ok(preview >= 0 && preview < daemon);
    assert.ok(daemon < stage && stage < manifest && manifest < adapter);
    assert.match(
      fs.readFileSync(
        path.join(extension.extensionPath, "crates/fleximarkd/src/main.rs"),
        "utf8",
      ),
      /include_str!\("\.\.\/\.\.\/\.\.\/web\/preview-client\/browser-host\.js"\)/,
    );
  });

  test("clean-installs the universal VSIX on every supported OS and CPU", () => {
    for (const workflowName of ["ci.yml", "release.yml"]) {
      const workflow = fs.readFileSync(
        path.join(extension.extensionPath, ".github/workflows", workflowName),
        "utf8",
      );
      for (const runner of [
        "ubuntu-24.04",
        "ubuntu-24.04-arm",
        "macos-15-intel",
        "macos-15",
        "windows-2025",
        "windows-11-arm",
      ])
        assert.match(workflow, new RegExp("os: " + runner));
      assert.match(workflow, /npm run smoke -- fleximark\.vsix/);
    }
    const smoke = fs.readFileSync(
      path.join(extension.extensionPath, "scripts/smoke-vsix.mjs"),
      "utf8",
    );
    assert.match(smoke, /createHash\("sha256"\)/);
    assert.match(smoke, /method: "fleximark\/initialize"/);
    assert.match(smoke, /result\?\.protocolVersion/);
  });

  test("verifies the bundled daemon checksum before normal startup", () => {
    const adapter = fs.readFileSync(
      path.join(extension.extensionPath, "adapters/vscode/src/adapter.mts"),
      "utf8",
    );
    assert.match(adapter, /#verifiedBundledDaemon/);
    assert.match(adapter, /checksum !== artifact\.sha256/);
  });
}
