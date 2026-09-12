from __future__ import annotations

import ast
import fnmatch
import hashlib
import json
import re
import subprocess
import sys
import tempfile
import tomllib
import unittest
from pathlib import Path
from unittest.mock import patch


ROOT = Path(__file__).resolve().parents[2]
SCRIPTS = ROOT / "scripts"
if str(SCRIPTS) not in sys.path:
    sys.path.insert(0, str(SCRIPTS))

import build as javascript_build
from _targets import TARGETS


def read(relative: str) -> str:
    return (ROOT / relative).read_text(encoding="utf-8")


def run(*command: str) -> subprocess.CompletedProcess[str]:
    result = subprocess.run(
        command,
        cwd=ROOT,
        capture_output=True,
        encoding="utf-8",
        errors="replace",
        check=False,
    )
    if result.returncode != 0:
        raise AssertionError(
            f"command failed ({result.returncode}): {' '.join(command)}\n"
            f"stdout:\n{result.stdout}\nstderr:\n{result.stderr}"
        )
    return result


def suite_members(entry: str, registrar: str) -> list[str]:
    source = read(entry)
    imports = {
        alias: module
        for alias, module in re.findall(
            r'import \* as (\w+) from "([^\"]+\.test\.mjs)";', source
        )
    }
    registrations = re.findall(
        rf"{registrar}\((\w+)\.suiteName,\s*\1\.suite\);", source
    )
    if len(registrations) != len(set(registrations)):
        raise AssertionError(f"duplicate suite registration in {entry}")
    if set(registrations) != set(imports):
        raise AssertionError(
            f"suite imports and registrations differ in {entry}: "
            f"imports={sorted(imports)}, registrations={sorted(registrations)}"
        )
    modules = [imports[alias] for alias in registrations]
    if len(modules) != len(set(modules)):
        raise AssertionError(f"duplicate suite module in {entry}")
    return modules


def workflow_job(workflow: str, name: str, next_name: str) -> str:
    start = workflow.index(f"  {name}:")
    end = workflow.index(f"  {next_name}:", start)
    return workflow[start:end]


def has_typescript_check(source: str) -> bool:
    return (
        re.search(
            r"\btsc\b[^\r\n]{0,200}(?:--noEmit|(?:['\"])?-b(?:['\"]|\s|$))",
            source,
        )
        is not None
    )


def ignored_by_vscodeignore(path: str, patterns: list[str]) -> bool:
    ignored = False
    for raw_pattern in patterns:
        negate = raw_pattern.startswith("!")
        pattern = raw_pattern[1:] if negate else raw_pattern
        if pattern.endswith("/**"):
            matched = path == pattern[:-3] or path.startswith(pattern[:-2])
        elif pattern.startswith("**/"):
            matched = fnmatch.fnmatchcase(path, pattern[3:]) or fnmatch.fnmatchcase(
                path, pattern
            )
        else:
            matched = fnmatch.fnmatchcase(path, pattern)
        if matched:
            ignored = not negate
    return ignored


def include_typescript_import_closure(paths: set[Path]) -> set[Path]:
    discovered = set(paths)
    pending = list(paths)
    import_pattern = re.compile(r'(?:from\s+|import\s*)["\']([^"\']+)["\']')
    while pending:
        owner = pending.pop()
        if not owner.is_file() or owner.suffix not in {".mts", ".ts"}:
            continue
        for specifier in import_pattern.findall(owner.read_text(encoding="utf-8")):
            if not specifier.startswith("."):
                continue
            imported = (owner.parent / specifier).resolve()
            candidates = [imported]
            if imported.suffix == ".mjs":
                candidates.append(imported.with_suffix(".mts"))
            elif imported.suffix == ".js":
                candidates.extend(
                    [imported.with_suffix(".ts"), imported.with_suffix(".mts")]
                )
            elif not imported.suffix:
                candidates.extend(
                    [imported.with_suffix(".ts"), imported.with_suffix(".mts")]
                )
            for candidate in candidates:
                if candidate.is_file() and candidate not in discovered:
                    discovered.add(candidate)
                    pending.append(candidate)
    return discovered


class TypeScriptTestBoundaryContract(unittest.TestCase):
    def test_pure_and_electron_entries_preserve_complete_unique_suite_membership(
        self,
    ) -> None:
        pure = suite_members("test/pure.test.mts", "describe")
        electron = suite_members("test/extension.test.mts", "suite")

        expected_pure = [
            "./adapter/rpc.test.mjs",
            "./adapter/daemon-supervisor.test.mjs",
            "./adapter/document-coordinator.test.mjs",
            "./adapter/preview-coordinator.test.mjs",
            "./adapter/release-manifest.test.mjs",
            "./browser-host.test.mjs",
            "./preview-client.test.mjs",
            "./vscode-host.test.mjs",
            "./protocol-contract.test.mjs",
        ]
        expected_electron = [
            "./contributions.test.mjs",
            "./adapter/daemon-runtime.test.mjs",
            "./adapter/document-lifecycle.test.mjs",
            "./adapter/export-ack.test.mjs",
            "./adapter/note-options.test.mjs",
            "./adapter/wiring.test.mjs",
            "./adapter/workspace-selection.test.mjs",
            "./adapter/multi-root-runtime.test.mjs",
            "./release.test.mjs",
        ]
        self.assertEqual(pure, expected_pure)
        self.assertEqual(electron, expected_electron)

        pure_set = set(pure)
        electron_set = set(electron)
        self.assertEqual(
            pure_set | electron_set, set(expected_pure + expected_electron)
        )
        self.assertTrue(
            set(expected_electron)
            - {"./adapter/rpc.test.mjs", "./preview-client.test.mjs"}
            <= electron_set,
            "Electron-only integration suites must not disappear during runner separation",
        )
        self.assertEqual(pure_set & electron_set, set())

    def test_release_runner_union_cannot_lose_the_existing_pure_boundary(self) -> None:
        pure = set(suite_members("test/pure.test.mts", "describe"))
        electron = set(suite_members("test/extension.test.mts", "suite"))
        release = read(".github/workflows/release.yml")
        validate = workflow_job(release, "validate", "release")
        coverage: set[str] = set()
        if "mise run test-pure" in validate:
            coverage.update(pure)
        if "mise run test -- --prebuilt" in validate:
            coverage.update(electron)

        self.assertEqual(pure & electron, set())
        self.assertIn("mise run test-pure", validate)
        self.assertIn("mise run test -- --prebuilt", validate)
        self.assertEqual(coverage, pure | electron)

    def test_vscode_test_discovery_selects_only_the_electron_artifact(self) -> None:
        config = read(".vscode-test.mjs")
        self.assertIn(
            'files: "out/test/electron/extension.test.cjs"',
            config,
        )
        extension_output = next(
            argument.removeprefix("--outfile=")
            for argument in javascript_build.test_args()
            if argument.startswith("--outfile=")
        )
        pure_output = next(
            argument.removeprefix("--outfile=")
            for argument in javascript_build.pure_test_args()
            if argument.startswith("--outfile=")
        )
        self.assertEqual(extension_output, "out/test/electron/extension.test.cjs")
        self.assertEqual(pure_output, "out/test/unit/pure-tests.cjs")
        self.assertTrue(extension_output.endswith(".test.cjs"))
        self.assertFalse(pure_output.endswith(".test.cjs"))


class JavaScriptEntrypointContract(unittest.TestCase):
    def test_unit_and_electron_builders_preserve_each_others_artifacts(self) -> None:
        with tempfile.TemporaryDirectory(prefix="fleximark-phase12-builders-") as temp:
            output = Path(temp)
            unit = output / "unit"
            electron = output / "electron"
            unit.mkdir()
            electron.mkdir()
            (unit / "keep.cjs").write_text("unit", encoding="utf-8")
            (electron / "stale.cjs").write_text("stale", encoding="utf-8")
            with (
                patch.object(javascript_build, "UNIT_TEST_OUTPUT", unit),
                patch.object(javascript_build, "ELECTRON_TEST_OUTPUT", electron),
                patch.object(javascript_build, "yarn") as yarn,
            ):
                javascript_build.build_electron_tests()
                self.assertTrue((unit / "keep.cjs").is_file())
                self.assertFalse((electron / "stale.cjs").exists())

                electron.mkdir(exist_ok=True)
                (electron / "keep.cjs").write_text("electron", encoding="utf-8")
                (unit / "stale.cjs").write_text("stale", encoding="utf-8")
                javascript_build.build_pure_tests()
                self.assertTrue((electron / "keep.cjs").is_file())
                self.assertFalse((unit / "stale.cjs").exists())
                self.assertEqual(yarn.call_count, 2)

    def test_developer_mise_and_ci_entrypoints_reach_the_same_tasks(self) -> None:
        package = json.loads(read("package.json"))
        mise = tomllib.loads(read("mise.toml"))
        self.assertEqual(
            {name: package["scripts"][name] for name in ["build", "test", "verify"]},
            {
                "build": "mise run build",
                "test": "mise run test",
                "verify": "mise run verify",
            },
        )
        self.assertEqual(package["scripts"]["package"], "mise run package --")
        self.assertEqual(
            package["scripts"]["vscode:prepublish"],
            "python scripts/tasks.py vscode-prepublish",
        )
        for task in [
            "build",
            "test",
            "test-pure",
            "verify",
            "package",
            "smoke",
        ]:
            self.assertEqual(
                mise["tasks"][task]["run"],
                f"uv run --frozen python scripts/tasks.py {task}",
            )

        ci = read(".github/workflows/ci.yml")
        for command in [
            "mise run test-pure",
            "mise run test -- --prebuilt",
            "yarn exec vsce package --no-dependencies --out fleximark.vsix",
            "uv run --frozen python scripts/release_artifact.py validate --vsix fleximark.vsix",
        ]:
            self.assertIn(command, ci)

    def test_public_and_bundle_entrypoints_are_exact(self) -> None:
        package = json.loads(read("package.json"))
        self.assertEqual(package["main"], "./dist/extension.cjs")
        self.assertEqual(
            javascript_build.extension_args(production=True),
            [
                "adapters/vscode/src/extension.mts",
                "--bundle",
                "--format=cjs",
                "--platform=node",
                "--outfile=dist/extension.cjs",
                "--external:vscode",
                "--loader:.css=text",
                "--define:__DEV__=false",
                "--minify",
            ],
        )
        self.assertEqual(
            javascript_build.preview_args(production=True),
            [
                "web/preview-client/vscode-host.mts",
                "web/preview-client/browser-host.mts",
                "--bundle",
                "--format=iife",
                "--platform=browser",
                "--outdir=dist/web/preview-client",
                "--minify",
            ],
        )

    def test_every_entrypoint_keeps_an_all_scope_typescript_check(self) -> None:
        tasks = read("scripts/tasks.py")
        dev = tasks[tasks.index("def dev(") : tasks.index("def compile_tests(")]
        verify = tasks[tasks.index("def verify(") : tasks.index("def package_vsix(")]
        ci = read(".github/workflows/ci.yml")
        release = read(".github/workflows/release.yml")
        release_validate = workflow_job(release, "validate", "release")
        for name, source in {
            "developer watch": dev,
            "local verify": verify,
            "CI validate": ci,
            "release validate": release_validate,
        }.items():
            with self.subTest(entrypoint=name):
                self.assertTrue(
                    has_typescript_check(source),
                    f"{name} must run either the current tsc --noEmit check or tsc -b",
                )
        for source in (tasks, ci, release):
            self.assertNotIn("--noEmit", source)

    def test_typescript_projects_cover_every_production_and_test_source(self) -> None:
        configs = [ROOT / "tsconfig.json"]
        visited: set[Path] = set()
        covered: set[Path] = set()
        membership: dict[Path, int] = {}
        while configs:
            config_path = configs.pop().resolve()
            if config_path in visited:
                continue
            visited.add(config_path)
            config = json.loads(config_path.read_text(encoding="utf-8"))
            base = config_path.parent
            for include in config.get("include", []):
                for path in base.glob(include):
                    resolved = path.resolve()
                    covered.add(resolved)
                    membership[resolved] = membership.get(resolved, 0) + 1
            for reference in config.get("references", []):
                referenced = (base / reference["path"]).resolve()
                configs.append(
                    referenced / "tsconfig.json" if referenced.is_dir() else referenced
                )

        covered = include_typescript_import_closure(covered)
        expected = {
            path.resolve()
            for owner in ("adapters", "web", "test")
            for path in (ROOT / owner).rglob("*.mts")
        }
        self.assertTrue(expected)
        self.assertEqual(
            {
                path.relative_to(ROOT).as_posix(): membership.get(path, 0)
                for path in expected
                if membership.get(path, 0) != 1
            },
            {},
            "every TypeScript source must belong directly to exactly one leaf project",
        )
        self.assertEqual(
            expected - covered,
            set(),
            "root include or referenced TypeScript projects must cover every source and test",
        )

    def test_package_rules_keep_runtime_assets_and_exclude_development_trees(
        self,
    ) -> None:
        package = json.loads(read("package.json"))
        patterns = [
            line.strip()
            for line in read(".vscodeignore").splitlines()
            if line.strip() and not line.lstrip().startswith("#")
        ]
        contribution_paths = {
            contribution["path"].removeprefix("./")
            for key in ("grammars", "snippets")
            for contribution in package["contributes"][key]
        }
        contribution_paths.update(
            language["configuration"].removeprefix("./")
            for language in package["contributes"]["languages"]
            if "configuration" in language
        )
        required = {
            package["main"].removeprefix("./"),
            package["icon"].removeprefix("./"),
            "dist/web/preview-client/vscode-host.js",
            "dist/web/preview-client/browser-host.js",
            "bin/manifest.json",
            *(target.bin_relative_path.as_posix() for target in TARGETS),
            *contribution_paths,
        }
        for path in sorted(required):
            with self.subTest(required=path):
                self.assertFalse(ignored_by_vscodeignore(path, patterns))

        forbidden = {
            "src/private.ts",
            "test/private.test.mts",
            "node_modules/dependency/index.js",
            "adapters/vscode/src/extension.mts",
            "web/preview-client/index.mts",
            "crates/fleximarkd/src/main.rs",
            "markdown_for_debug/example.md",
            "parserPlugin.js",
            ".ruff_cache/CACHEDIR.TAG",
            "nested/tsconfig.unit.json",
            "out/types/unit.tsbuildinfo",
        }
        for path in sorted(forbidden):
            with self.subTest(forbidden=path):
                self.assertTrue(ignored_by_vscodeignore(path, patterns))


class CargoBoundaryContract(unittest.TestCase):
    def test_every_non_formatting_cargo_entrypoint_is_locked(self) -> None:
        for relative in ("scripts/tasks.py", "scripts/check_performance_budgets.py"):
            tree = ast.parse(read(relative), filename=relative)
            commands = []
            for node in ast.walk(tree):
                if not isinstance(node, ast.Call) or not node.args:
                    continue
                arguments = [
                    argument.value
                    for argument in node.args
                    if isinstance(argument, ast.Constant)
                    and isinstance(argument.value, str)
                ]
                if arguments and arguments[0] == "cargo":
                    commands.append(arguments)
            self.assertTrue(commands, relative)
            for command in commands:
                with self.subTest(owner=relative, command=command):
                    if len(command) > 1 and command[1] != "fmt":
                        self.assertIn("--locked", command)

        for workflow in (".github/workflows/ci.yml", ".github/workflows/release.yml"):
            cargo_commands = [
                line.strip().removeprefix("run: ")
                for line in read(workflow).splitlines()
                if line.strip().startswith("run: cargo")
            ]
            self.assertTrue(cargo_commands, workflow)
            for command in cargo_commands:
                with self.subTest(owner=workflow, command=command):
                    if " cargo fmt " not in f" {command} ":
                        self.assertIn("--locked", command)

        for relative in (
            "crates/fleximark-engine/src/tests.rs",
            "crates/fleximark-plugin-host/src/tests.rs",
        ):
            source = read(relative)
            self.assertEqual(source.count('Command::new(env!("CARGO"))'), 1)
            command = source[
                source.index('Command::new(env!("CARGO"))') : source.index(
                    ".status()", source.index('Command::new(env!("CARGO"))')
                )
            ]
            self.assertIn('"--manifest-path"', command)
            self.assertIn('"wasm32-wasip2"', command)
            self.assertEqual(command.count('"--locked"'), 1)

    def test_metadata_preserves_msrv_base64_and_lock_resolution(self) -> None:
        lock_path = ROOT / "Cargo.lock"
        before = hashlib.sha256(lock_path.read_bytes()).digest()
        metadata = json.loads(
            run("cargo", "metadata", "--format-version", "1", "--locked").stdout
        )
        after = hashlib.sha256(lock_path.read_bytes()).digest()
        self.assertEqual(after, before)

        workspace_ids = set(metadata["workspace_members"])
        workspace_packages = [
            package
            for package in metadata["packages"]
            if package["id"] in workspace_ids
        ]
        self.assertTrue(workspace_packages)
        self.assertTrue(
            all(package["rust_version"] == "1.85" for package in workspace_packages)
        )
        self.assertTrue(
            all(package["edition"] == "2024" for package in workspace_packages)
        )

        base64_packages = [
            package for package in metadata["packages"] if package["name"] == "base64"
        ]
        self.assertEqual(
            [(package["version"], package["source"]) for package in base64_packages],
            [("0.22.1", "registry+https://github.com/rust-lang/crates.io-index")],
        )
        daemon = next(
            package for package in workspace_packages if package["name"] == "fleximarkd"
        )
        daemon_base64 = [
            dependency
            for dependency in daemon["dependencies"]
            if dependency["name"] == "base64" and dependency["kind"] is None
        ]
        self.assertEqual(len(daemon_base64), 1)
        self.assertEqual(daemon_base64[0]["req"], "^0.22.1")
        daemon_manifest = tomllib.loads(read("crates/fleximarkd/Cargo.toml"))
        self.assertEqual(daemon_manifest["dependencies"]["base64"], {"workspace": True})
        fixture_manifest = tomllib.loads(read("fixtures/plugin-component/Cargo.toml"))
        self.assertEqual(fixture_manifest["package"]["rust-version"], "1.85")

        cli = next(
            package
            for package in workspace_packages
            if package["name"] == "fleximark-cli"
        )
        self.assertNotIn(
            "fleximark-parser",
            {dependency["name"] for dependency in cli["dependencies"]},
        )

        lock = tomllib.loads(lock_path.read_text(encoding="utf-8"))
        locked_base64 = [
            package for package in lock["package"] if package["name"] == "base64"
        ]
        self.assertEqual(len(locked_base64), 1)
        self.assertEqual(
            {key: locked_base64[0][key] for key in ("name", "version", "source")},
            {
                "name": "base64",
                "version": "0.22.1",
                "source": "registry+https://github.com/rust-lang/crates.io-index",
            },
        )

    def test_cli_all_targets_compile_without_direct_parser_api(self) -> None:
        cli = ROOT / "crates" / "fleximark-cli"
        rust_targets = sorted(cli.rglob("*.rs"))
        self.assertTrue(rust_targets)
        for target in rust_targets:
            self.assertNotIn(
                "fleximark_parser",
                target.read_text(encoding="utf-8"),
                target.relative_to(ROOT).as_posix(),
            )

        lock_path = ROOT / "Cargo.lock"
        before = hashlib.sha256(lock_path.read_bytes()).digest()
        tree = run(
            "cargo",
            "tree",
            "-p",
            "fleximark-cli",
            "-e",
            "normal",
            "--prefix",
            "none",
            "--locked",
        ).stdout
        self.assertRegex(tree, r"(?m)^fleximark-parser v0\.1\.0 ")
        run(
            "cargo",
            "check",
            "--locked",
            "-p",
            "fleximark-cli",
            "--all-targets",
        )
        self.assertEqual(hashlib.sha256(lock_path.read_bytes()).digest(), before)

    def test_current_rust_validation_uses_the_declared_workspace_scope(self) -> None:
        workspace = tomllib.loads(read("Cargo.toml"))
        self.assertEqual(workspace["workspace"]["package"]["rust-version"], "1.85")
        ci = read(".github/workflows/ci.yml")
        release = read(".github/workflows/release.yml")
        validation_jobs = {
            ".github/workflows/ci.yml": workflow_job(
                ci, "validate", "daemon-platforms"
            ),
            ".github/workflows/release.yml": workflow_job(
                release, "validate", "release"
            ),
        }
        for workflow, source in validation_jobs.items():
            with self.subTest(workflow=workflow):
                self.assertIn(
                    "rustup toolchain install 1.85.0 --profile minimal", source
                )
                self.assertIn("rustup +1.85.0 target add wasm32-wasip2", source)
                self.assertIn(
                    "cargo +1.85.0 check --manifest-path fixtures/plugin-component/Cargo.toml --target wasm32-wasip2 --locked",
                    source,
                )
                self.assertRegex(
                    source,
                    r"(?s)CARGO_TARGET_DIR: target/msrv-fixture-1\.85\s+run: cargo \+1\.85\.0 check --manifest-path fixtures/plugin-component/Cargo\.toml --target wasm32-wasip2 --locked",
                )
                self.assertIn(
                    "cargo +1.85.0 check --workspace --all-targets --locked",
                    source,
                )
                self.assertLess(
                    source.index("Build external preview client"),
                    source.index(
                        "cargo +1.85.0 check --workspace --all-targets --locked"
                    ),
                    "the generated preview client must exist before Rust include_str! checks",
                )
                self.assertRegex(
                    source,
                    r"(?s)CARGO_TARGET_DIR: target/msrv-1\.85\s+run: cargo \+1\.85\.0 check --workspace --all-targets --locked",
                )
                self.assertGreaterEqual(
                    source.count("rustup target add wasm32-wasip2"),
                    1,
                    "current-toolchain fixture tests need their own wasm target",
                )
                self.assertIn("cargo test --workspace --all-targets --locked", source)
                self.assertIn(
                    "cargo clippy --workspace --all-targets --locked -- -D warnings",
                    source,
                )

        daemon_platform_jobs = {
            ".github/workflows/ci.yml": workflow_job(
                ci, "daemon-platforms", "clean-install"
            ),
            ".github/workflows/release.yml": workflow_job(
                release, "daemon-platforms", "validate"
            ),
        }
        for workflow, source in daemon_platform_jobs.items():
            with self.subTest(workflow=workflow, job="daemon-platforms"):
                self.assertEqual(source.count("os:"), 6)
                self.assertIn(
                    "rustup toolchain install 1.85.0 --profile minimal", source
                )
                self.assertRegex(
                    source,
                    r"(?s)CARGO_TARGET_DIR: target/msrv-1\.85\s+run: cargo \+1\.85\.0 check --workspace --all-targets --locked",
                )
                self.assertLess(
                    source.index("Build external preview client"),
                    source.index(
                        "cargo +1.85.0 check --workspace --all-targets --locked"
                    ),
                    "the generated preview client must exist before Rust include_str! checks",
                )
                self.assertNotIn("rustup +1.85.0 target add", source)
                self.assertIn("cargo build --release -p fleximarkd --locked", source)

        release_job = workflow_job(release, "release", "clean-install")
        self.assertRegex(
            release_job,
            r"(?m)^    needs: \[[^\]]*\bvalidate\b[^\]]*\]$",
            "publishing must depend on the complete release validation job",
        )

        dependabot = read(".github/dependabot.yml")
        cargo_directories = re.findall(
            r"(?m)^  - package-ecosystem: cargo\r?\n    directory: (.+)$",
            dependabot,
        )
        self.assertEqual(cargo_directories, ["/", "/fixtures/plugin-component"])


if __name__ == "__main__":
    unittest.main()
