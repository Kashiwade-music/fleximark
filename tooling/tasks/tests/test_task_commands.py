from __future__ import annotations

import io
import json
import sys
import subprocess as subprocess_module
import tomllib
import unittest
from contextlib import ExitStack, redirect_stderr, redirect_stdout
from pathlib import Path
from unittest.mock import Mock, patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import tasks
import build as javascript_build
import task_checks
import task_js
import task_plan
import task_products
import task_workflows
from task_harness import action


class TaskCommandTests(unittest.TestCase):
    def run_cli(self, *arguments: str) -> str:
        output = io.StringIO()
        with redirect_stdout(output):
            tasks.main(arguments)
        return output.getvalue()

    def test_missing_build_package_or_generation_target_fails_before_any_action(self):
        for command in ("build", "package", "generate"):
            with self.subTest(command=command), action("create_plan") as plan:
                with redirect_stderr(io.StringIO()), self.assertRaises(SystemExit) as error:
                    tasks.main([command])
                self.assertEqual(error.exception.code, 2)
                plan.assert_not_called()

    def test_invalid_arguments_and_removed_aliases_fail_before_any_action(self):
        for arguments in (
            ["build", "vscode", "--typo"], ["test", "unknown"], ["smoke"],
            ["build-all"], ["dev"], ["test-pure"], ["verify-quality"],
        ):
            with self.subTest(arguments=arguments), action("create_plan") as plan:
                with redirect_stderr(io.StringIO()), self.assertRaises(SystemExit):
                    tasks.main(arguments)
                plan.assert_not_called()

    def test_dry_run_lists_every_command_without_invoking_actions_or_subprocesses(self):
        commands = [
            ["setup"], ["watch"], ["check"], ["test"], ["test", "editor"],
            ["verify"], ["audit"], ["smoke", "a path with spaces/product.vsix"],
            ["clean"], ["ci", "matrix"], ["bundle", "tests"], ["contract", "check"],
            ["distribution", "runtime"], ["artifact", "validate"],
            ["release-assets", "--root", ".artifacts", "--source-git-head", "a" * 40, "--tag", "v1.2.3"],
            *[["build", target] for target in ("core", "runtime", "vscode", "zed", "all")],
            *[["package", target] for target in ("runtime", "vscode", "zed", "all")],
            *[["generate", target] for target in ("protocol", "l10n", "versions", "architecture", "all")],
            *[["ci", phase] for phase in ("quality", "product", "zed")],
        ]
        with ExitStack() as stack:
            actions = [stack.enter_context(action(name)) for name in (
                "run", "yarn", "watch_vscode", "build_daemon", "build_zed_product",
                "build_vscode_product", "assemble_runtime", "generate_versions",
                "generate_protocol_contract", "check_protocol_contract", "check_architecture",
                "python_test", "pure_test", "check_localization", "export_localization",
                "clean", "smoke_vsix", "verify_built_product",
                "package_identified_product",
            )]
            actions += [stack.enter_context(patch.object(javascript_build, name)) for name in (
                "build_browser_client", "build_workspace_ui",
            )]
            subprocess = stack.enter_context(patch.object(subprocess_module, "run"))
            for command in commands:
                with self.subTest(command=command):
                    output = self.run_cli(*command, "--dry-run")
                    self.assertIn("no steps will run", output)
                    self.assertIn("1.", output)
            for mocked_action in [*actions, subprocess]:
                mocked_action.assert_not_called()

    def test_vscode_build_and_package_never_build_zed_or_run_tests(self):
        for command in ("build", "package"):
            with self.subTest(command=command), ExitStack() as stack:
                for name in ("generate_versions", "check_protocol_contract", "assemble_runtime", "build_daemon"):
                    stack.enter_context(action(name))
                for name in ("build_browser_client", "build_workspace_ui"):
                    stack.enter_context(patch.object(javascript_build, name))
                vscode = stack.enter_context(action("build_vscode_product"))
                zed = stack.enter_context(action("build_zed_product"))
                package = stack.enter_context(action("package_identified_product"))
                tests = [stack.enter_context(action(name)) for name in (
                    "python_test", "pure_test", "run_editor_integration",
                    "verify_built_product", "verify_audit", "smoke_vsix", "check_performance_budgets",
                )]
                self.run_cli(command, "vscode")
                vscode.assert_called_once()
                zed.assert_not_called()
                self.assertEqual(package.call_count, int(command == "package"))
                for test in tests:
                    test.assert_not_called()

    def test_package_all_creates_identity_without_launching_test_gates(self):
        with action("build_steps", return_value=[]), action("package_identified_product") as package:
            with ExitStack() as stack:
                gates = [stack.enter_context(action(name)) for name in (
                    "check_performance_budgets", "compile_tests", "run_editor_integration", "smoke_vsix",
                )]
                self.run_cli("package", "all")
        package.assert_called_once_with(())
        for gate in gates:
            gate.assert_not_called()

    def test_setup_prepares_both_locked_rust_workspaces_after_toolchains(self):
        with action("run") as run:
            self.run_cli("setup")
        commands = [call.args for call in run.call_args_list]
        self.assertEqual(commands, [
            ("git", "submodule", "update", "--init", "--recursive"),
            ("yarn", "install", "--immutable"),
            ("rustup", "component", "add", "rustfmt", "clippy"),
            ("rustup", "target", "add", "wasm32-wasip2"),
            ("cargo", "fetch", "--locked"),
            ("cargo", "fetch", "--locked", "--manifest-path", "adapters/zed/Cargo.toml"),
        ])

    def test_setup_stops_before_zed_fetch_if_root_fetch_fails(self):
        def execute(*command):
            if command == ("cargo", "fetch", "--locked"):
                raise RuntimeError("root dependency fetch failed")

        with action("run", side_effect=execute) as run:
            with self.assertRaisesRegex(RuntimeError, "root dependency fetch failed"):
                self.run_cli("setup")
        self.assertEqual(run.call_args_list[-1].args, ("cargo", "fetch", "--locked"))

    def test_default_test_runs_all_non_gui_suites_without_packaging(self):
        with ExitStack() as stack:
            run = stack.enter_context(action("run"))
            stack.enter_context(action("check_protocol_contract"))
            python = stack.enter_context(action("python_test"))
            node = stack.enter_context(action("pure_test"))
            editor = stack.enter_context(action("run_editor_integration"))
            package = stack.enter_context(action("package_identified_product"))
            self.run_cli("test")
        python.assert_called_once()
        node.assert_called_once()
        cargo_tests = [call.args for call in run.call_args_list]
        self.assertEqual(len(cargo_tests), 2)
        self.assertTrue(all(command[:2] == ("cargo", "test") for command in cargo_tests))
        self.assertTrue(any("--workspace" in command for command in cargo_tests))
        for owner in ("adapters/zed/Cargo.toml",):
            self.assertTrue(any(owner in command for command in cargo_tests))
        editor.assert_not_called()
        package.assert_not_called()

    def test_check_never_executes_test_suites_or_packages(self):
        with ExitStack() as stack:
            run = stack.enter_context(action("run"))
            stack.enter_context(action("yarn"))
            for name in ("check_generated_freshness", "check_protocol_contract", "check_localization"):
                stack.enter_context(action(name))
            suites = [stack.enter_context(action(name)) for name in (
                "python_test", "pure_test", "run_editor_integration", "package_identified_product",
            )]
            self.run_cli("check")
        self.assertFalse(any(call.args[:2] == ("cargo", "test") for call in run.call_args_list))
        for suite in suites:
            suite.assert_not_called()

    def test_execution_stops_before_packaging_if_a_build_step_fails(self):
        with action("check_protocol_contract", side_effect=RuntimeError("stale protocol")), action("package_identified_product") as package:
            with self.assertRaisesRegex(RuntimeError, "stale protocol"):
                self.run_cli("package", "vscode")
            package.assert_not_called()

    def test_plan_stops_immediately_on_a_failed_step(self):
        later = Mock()
        failure = Mock(side_effect=RuntimeError("failed"))
        with redirect_stdout(io.StringIO()), self.assertRaises(RuntimeError):
            task_plan.Plan([task_plan.Step("fails", failure), task_plan.Step("must not run", later)], "done").execute()
        later.assert_not_called()

    def test_mise_is_the_single_public_wrapper(self):
        root = Path(__file__).resolve().parents[3]
        package = json.loads((root / "package.json").read_text(encoding="utf-8"))
        self.assertNotIn("scripts", package)
        config = tomllib.loads((root / "mise.toml").read_text(encoding="utf-8"))["tasks"]
        for name, value in config.items():
            self.assertEqual(value["run"], "python tooling/tasks/tasks.py" if name in {"default", "help"} else f"python tooling/tasks/tasks.py {name}")



if __name__ == "__main__":
    unittest.main()
