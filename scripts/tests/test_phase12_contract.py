from __future__ import annotations

import ast
import fnmatch
import hashlib
import json
import re
import subprocess
import sys
import unittest
from pathlib import Path


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
            "./adapter/workspace-migration.test.mjs",
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
            "./adapter/workspace-migration-runtime.test.mjs",
            "./adapter/multi-root-runtime.test.mjs",
        ]
        self.assertEqual(pure, expected_pure)
        self.assertEqual(electron, expected_electron)

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


class JavaScriptEntrypointContract(unittest.TestCase):
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
        owners = ("scripts/tasks.py", "scripts/check_performance_budgets.py")
        for relative in owners:
            tree = ast.parse(read(relative), filename=relative)
            commands: list[list[str]] = []
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
            cargo_commands = re.findall(r"^\s*run: (cargo .+)$", read(workflow), re.M)
            self.assertTrue(cargo_commands, workflow)
            for command in cargo_commands:
                with self.subTest(owner=workflow, command=command):
                    if not command.startswith("cargo fmt "):
                        self.assertIn("--locked", command)

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
            all(package["rust_version"] == "1.86" for package in workspace_packages)
        )
        self.assertTrue(
            all(package["edition"] == "2024" for package in workspace_packages)
        )

        base64_packages = [p for p in metadata["packages"] if p["name"] == "base64"]
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

if __name__ == "__main__":
    unittest.main()
