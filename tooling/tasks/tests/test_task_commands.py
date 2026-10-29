from __future__ import annotations

import io
import json
import sys
import tomllib
import unittest
from contextlib import ExitStack, redirect_stderr, redirect_stdout
from pathlib import Path
from unittest.mock import Mock, patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import tasks


class TaskCommandTests(unittest.TestCase):
    def run_cli(self, *arguments: str) -> str:
        output = io.StringIO()
        with redirect_stdout(output):
            tasks.main(arguments)
        return output.getvalue()

    def test_missing_build_package_or_generation_target_fails_before_any_action(self):
        for command in ("build", "package", "generate"):
            with self.subTest(command=command), patch.object(tasks, "create_plan") as plan:
                with redirect_stderr(io.StringIO()), self.assertRaises(SystemExit) as error:
                    tasks.main([command])
                self.assertEqual(error.exception.code, 2)
                plan.assert_not_called()

    def test_invalid_arguments_and_removed_aliases_fail_before_any_action(self):
        for arguments in (
            ["build", "vscode", "--typo"], ["test", "unknown"], ["smoke"],
            ["build-all"], ["dev"], ["test-pure"], ["verify-quality"],
        ):
            with self.subTest(arguments=arguments), patch.object(tasks, "create_plan") as plan:
                with redirect_stderr(io.StringIO()), self.assertRaises(SystemExit):
                    tasks.main(arguments)
                plan.assert_not_called()

    def test_dry_run_lists_every_command_without_invoking_actions_or_subprocesses(self):
        commands = [
            ["setup"], ["watch"], ["check"], ["test"], ["test", "editor"],
            ["verify"], ["audit"], ["smoke", "a path with spaces/product.vsix"],
            ["clean"], ["vscode-prepublish"],
            *[["build", target] for target in ("core", "runtime", "vscode", "zed", "all")],
            *[["package", target] for target in ("runtime", "vscode", "zed", "all")],
            *[["generate", target] for target in ("protocol", "l10n", "versions", "architecture", "all")],
            *[["ci", phase] for phase in ("quality", "product", "zed")],
        ]
        with ExitStack() as stack:
            actions = [stack.enter_context(patch.object(tasks, name)) for name in (
                "run", "yarn", "watch_vscode", "build_daemon", "build_zed_product",
                "build_vscode_product", "assemble_runtime", "generate_versions",
                "generate_protocol_contract", "check_protocol_contract", "check_architecture",
                "python_test", "pure_test", "check_localization", "export_localization",
                "clean", "smoke_vsix", "verify_built_product", "vscode_prepublish",
                "integration_test", "package_built_product",
            )]
            actions += [stack.enter_context(patch.object(tasks.javascript_build, name)) for name in (
                "build_browser_client", "build_workspace_ui",
            )]
            subprocess = stack.enter_context(patch.object(tasks.subprocess, "run"))
            for command in commands:
                with self.subTest(command=command):
                    output = self.run_cli(*command, "--dry-run")
                    self.assertIn("no steps will run", output)
                    self.assertIn("1.", output)
            for action in [*actions, subprocess]:
                action.assert_not_called()

    def test_vscode_build_and_package_never_build_zed_or_run_tests(self):
        for command in ("build", "package"):
            with self.subTest(command=command), ExitStack() as stack:
                for name in ("generate_versions", "check_protocol_contract", "assemble_runtime", "build_daemon"):
                    stack.enter_context(patch.object(tasks, name))
                for name in ("build_browser_client", "build_workspace_ui"):
                    stack.enter_context(patch.object(tasks.javascript_build, name))
                vscode = stack.enter_context(patch.object(tasks, "build_vscode_product"))
                zed = stack.enter_context(patch.object(tasks, "build_zed_product"))
                package = stack.enter_context(patch.object(tasks, "package_built_product"))
                tests = [stack.enter_context(patch.object(tasks, name)) for name in (
                    "python_test", "pure_test", "integration_test", "run_editor_integration",
                    "verify_built_product", "verify_audit", "smoke_vsix",
                )]
                self.run_cli(command, "vscode")
                vscode.assert_called_once()
                zed.assert_not_called()
                self.assertEqual(package.call_count, int(command == "package"))
                for test in tests:
                    test.assert_not_called()

    def test_default_test_runs_all_non_gui_suites_without_packaging(self):
        with ExitStack() as stack:
            run = stack.enter_context(patch.object(tasks, "run"))
            python = stack.enter_context(patch.object(tasks, "python_test"))
            node = stack.enter_context(patch.object(tasks, "pure_test"))
            editor = stack.enter_context(patch.object(tasks, "run_editor_integration"))
            package = stack.enter_context(patch.object(tasks, "package_built_product"))
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
            run = stack.enter_context(patch.object(tasks, "run"))
            stack.enter_context(patch.object(tasks, "yarn"))
            for name in ("check_generated_freshness", "check_protocol_contract", "check_localization"):
                stack.enter_context(patch.object(tasks, name))
            suites = [stack.enter_context(patch.object(tasks, name)) for name in (
                "python_test", "pure_test", "run_editor_integration", "package_built_product",
            )]
            self.run_cli("check")
        self.assertFalse(any(call.args[:2] == ("cargo", "test") for call in run.call_args_list))
        for suite in suites:
            suite.assert_not_called()

    def test_execution_stops_before_packaging_if_a_build_step_fails(self):
        with patch.object(tasks, "check_protocol_contract", side_effect=RuntimeError("stale protocol")), patch.object(tasks, "package_built_product") as package:
            with self.assertRaisesRegex(RuntimeError, "stale protocol"):
                self.run_cli("package", "vscode")
            package.assert_not_called()

    def test_plan_stops_immediately_on_a_failed_step(self):
        later = Mock()
        failure = Mock(side_effect=RuntimeError("failed"))
        with redirect_stdout(io.StringIO()), self.assertRaises(RuntimeError):
            tasks.Plan([tasks.Step("fails", failure), tasks.Step("must not run", later)], "done").execute()
        later.assert_not_called()

    def test_yarn_and_mise_share_command_meanings_and_forward_arguments(self):
        root = Path(__file__).resolve().parents[3]
        scripts = json.loads((root / "package.json").read_text(encoding="utf-8"))["scripts"]
        config = tomllib.loads((root / "mise.toml").read_text(encoding="utf-8"))["tasks"]
        for name, script in scripts.items():
            self.assertEqual(script, f"mise run {name}")
            self.assertEqual(config[name]["run"], f"python tooling/tasks/tasks.py {name}")


if __name__ == "__main__":
    unittest.main()
