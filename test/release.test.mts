import * as assert from "node:assert/strict";
import * as fs from "node:fs";
import * as path from "node:path";
import * as vscode from "vscode";

export const suiteName = "Release assembly";

export function suite(): void {
  const extension = vscode.extensions.getExtension("Kashiwade.fleximark");
  assert.ok(extension);

  test("pins the mise Yarn and uv development toolchain", () => {
    const packageJson = JSON.parse(
      fs.readFileSync(
        path.join(extension.extensionPath, "package.json"),
        "utf8",
      ),
    );
    assert.equal(packageJson.packageManager, "yarn@4.18.0");
    assert.equal(packageJson.devEngines.packageManager.name, "yarn");
    for (const file of [
      ".yarnrc.yml",
      "mise.toml",
      "pyproject.toml",
      "uv.lock",
      "yarn.lock",
    ])
      assert.ok(fs.existsSync(path.join(extension.extensionPath, file)), file);
    assert.ok(
      !fs.existsSync(path.join(extension.extensionPath, "package-lock.json")),
    );
  });

  test("owns the six daemon artifact targets in one Python module", () => {
    const targetSource = fs.readFileSync(
      path.join(extension.extensionPath, "scripts/_targets.py"),
      "utf8",
    );
    const declarations = [
      ...targetSource.matchAll(/Target\("([^"]+)", "([^"]+)", "([^"]+)"\)/g),
    ].map(([, platform, arch, executable]) => [platform, arch, executable]);
    assert.deepEqual(declarations, [
      ["linux", "x64", "fleximarkd"],
      ["linux", "arm64", "fleximarkd"],
      ["darwin", "x64", "fleximarkd"],
      ["darwin", "arm64", "fleximarkd"],
      ["win32", "x64", "fleximarkd.exe"],
      ["win32", "arm64", "fleximarkd.exe"],
    ]);
    assert.match(
      targetSource,
      /PurePosixPath\("bin"\) \/ f"\{self\.platform\}-\{self\.arch\}" \/ self\.executable/,
    );

    const manifestSource = fs.readFileSync(
      path.join(extension.extensionPath, "scripts/create_release_manifest.py"),
      "utf8",
    );
    assert.match(manifestSource, /from _targets import TARGETS/);
    assert.match(manifestSource, /target\.bin_relative_path\.as_posix\(\)/);
    assert.doesNotMatch(manifestSource, /^TARGETS\s*=/m);
  });

  test("implements custom development tasks in Python", () => {
    const scripts = path.join(extension.extensionPath, "scripts");
    const pythonScripts = fs
      .readdirSync(scripts)
      .filter((name) => name.endsWith(".py"));
    for (const name of [
      "build.py",
      "_targets.py",
      "check_performance_budgets.py",
      "create_release_manifest.py",
      "l10n_export.py",
      "release_artifact.py",
      "smoke_vsix.py",
      "stage_daemon.py",
      "tasks.py",
      "verify_architecture.py",
    ])
      assert.ok(pythonScripts.includes(name), name);
    assert.deepEqual(
      fs.readdirSync(scripts).filter((name) => name.endsWith(".mjs")),
      ["semantic-release-package.mjs"],
    );
  });

  test("assembles daemons before semantic release packaging", () => {
    const workflow = fs.readFileSync(
      path.join(extension.extensionPath, ".github/workflows/release.yml"),
      "utf8",
    );
    assert.match(workflow, /pattern: daemon-\*/);
    assert.match(workflow, /create_release_manifest\.py --require-all/);
    assert.match(workflow, /build\.py extension --production/);
  });

  test("builds the external preview client before compiling the daemon", () => {
    const packageJson = JSON.parse(
      fs.readFileSync(
        path.join(extension.extensionPath, "package.json"),
        "utf8",
      ),
    );
    assert.equal(packageJson.scripts.build, "mise run build");
    const tasks = fs.readFileSync(
      path.join(extension.extensionPath, "scripts/tasks.py"),
      "utf8",
    );
    const preview = tasks.indexOf("javascript_build.build_browser_client()");
    const daemon = tasks.indexOf('run("cargo", "build", "--release"');
    const stage = tasks.indexOf("stage_daemon()");
    const manifest = tasks.indexOf("create_manifest()");
    const adapter = tasks.indexOf(
      "javascript_build.build_extension(production=True)",
    );
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
      assert.match(workflow, /mise run smoke -- fleximark\.vsix/);
    }
    const smoke = fs.readFileSync(
      path.join(extension.extensionPath, "scripts/smoke_vsix.py"),
      "utf8",
    );
    assert.match(smoke, /hashlib\.sha256/);
    assert.match(smoke, /"method": "fleximark\/initialize"/);
    assert.match(smoke, /result\.get\("protocolVersion"\)/);
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
