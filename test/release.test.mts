import * as assert from "node:assert/strict";
import * as fs from "node:fs";
import * as path from "node:path";
import * as vscode from "vscode";

export const suiteName = "Release assembly";

export function suite(): void {
  const extension = vscode.extensions.getExtension("Kashiwade.fleximark");
  assert.ok(extension);

  test("declares Windows macOS and Linux x64 daemon artifacts", () => {
    const source = fs.readFileSync(
      path.join(extension.extensionPath, "scripts/create-release-manifest.mjs"),
      "utf8",
    );
    for (const target of ["win32", "darwin", "linux"])
      assert.match(source, new RegExp('\\["' + target + '", "x64"'));
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
    const adapter = tasks.indexOf('runNode("esbuild.js", "--production")');
    assert.ok(preview >= 0 && preview < daemon);
    assert.ok(daemon < stage && stage < adapter);
    assert.match(
      fs.readFileSync(
        path.join(extension.extensionPath, "crates/fleximarkd/src/main.rs"),
        "utf8",
      ),
      /include_str!\("\.\.\/\.\.\/\.\.\/web\/preview-client\/browser-host\.js"\)/,
    );
  });

  test("clean-installs the universal VSIX on every supported OS", () => {
    for (const workflowName of ["ci.yml", "release.yml"]) {
      const workflow = fs.readFileSync(
        path.join(extension.extensionPath, ".github/workflows", workflowName),
        "utf8",
      );
      assert.match(workflow, /os: \[ubuntu-latest, macos-13, windows-latest\]/);
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
}
