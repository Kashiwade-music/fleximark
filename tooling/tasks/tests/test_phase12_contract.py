from __future__ import annotations

import ast
import hashlib
import json
import re
import subprocess
import sys
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[3]
SCRIPTS = ROOT / "tooling/tasks"
if str(SCRIPTS) not in sys.path:
    sys.path.insert(0, str(SCRIPTS))



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
        pure = suite_members("tests/typescript/pure.test.mts", "describe")
        electron = suite_members("tests/typescript/extension.test.mts", "suite")

        expected_pure = [
            "./adapter/rpc.test.mjs",
            "./adapter/adapter-preview-events.test.mjs",
            "./adapter/daemon-supervisor.test.mjs",
            "./adapter/document-coordinator.test.mjs",
            "./adapter/note-options.test.mjs",
            "./adapter/preview-coordinator.test.mjs",
            "./adapter/release-manifest.test.mjs",
            "./browser-host.test.mjs",
            "./preview-client.test.mjs",
            "./vscode-host.test.mjs",
            "./protocol-contract.test.mjs",
            "./workspace-ui.test.mjs",
        ]
        expected_electron = [
            "./contributions.test.mjs",
            "./adapter/daemon-runtime.test.mjs",
            "./adapter/browser-navigation-runtime.test.mjs",
            "./adapter/document-lifecycle.test.mjs",
            "./adapter/export-ack.test.mjs",
            "./adapter/wiring.test.mjs",
            "./adapter/workspace-selection.test.mjs",
            "./adapter/multi-root-runtime.test.mjs",
        ]
        self.assertEqual(pure, expected_pure)
        self.assertEqual(electron, expected_electron)

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
            for owner in ("adapters/vscode", "web", "contracts/typescript", "tests/typescript")
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

class CargoBoundaryContract(unittest.TestCase):
    def test_every_non_formatting_cargo_entrypoint_is_locked(self) -> None:
        owners = (
            "tooling/tasks/tasks.py",
            "tooling/tasks/check_performance_budgets.py",
            "tooling/tasks/protocol_codegen.py",
        )
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
                    if len(command) > 1 and command[1] not in {"fmt", "xtask"}:
                        self.assertIn("--locked", command)

        actions = "\n".join(
            path.read_text(encoding="utf-8")
            for path in (ROOT / ".github/actions").glob("*/action.yml")
        )
        for workflow in (".github/workflows/ci.yml", ".github/workflows/release.yml"):
            cargo_commands = re.findall(
                r"^\s*(?:run: )?(?:CARGO_TARGET_DIR=\S+ )?(cargo(?: \+\S+)? .+)$",
                read(workflow) + actions,
                re.M,
            )
            self.assertTrue(cargo_commands, workflow)
            for command in cargo_commands:
                with self.subTest(owner=workflow, command=command):
                    if not command.startswith(("cargo fmt ", "cargo xtask ")):
                        self.assertIn("--locked", command)

    def test_workspace_metadata_keeps_lock_and_declares_rust_requirements(self) -> None:
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
        self.assertTrue(all(package["rust_version"] is not None for package in workspace_packages))
        self.assertTrue(all(package["edition"] == "2024" for package in workspace_packages))
        self.assertIn("fleximark-model", {p["name"] for p in workspace_packages})
        self.assertIn("fleximark-protocol", {p["name"] for p in workspace_packages})

if __name__ == "__main__":
    unittest.main()
