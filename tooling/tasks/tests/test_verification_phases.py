from __future__ import annotations

import sys
import tempfile
import unittest
from contextlib import ExitStack
from pathlib import Path
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import build
import tasks
import build as javascript_build
import task_checks
import task_js
import task_plan
import task_products
import task_workflows
from task_harness import action


class VerificationPhasesTests(unittest.TestCase):
    def test_runtime_browser_is_built_once_and_extension_only_builds_its_host(self):
        with ExitStack() as stack:
            browser = stack.enter_context(patch.object(build, "build_browser_client"))
            calls = stack.enter_context(patch.object(build, "yarn"))
            for name in ("generate_versions", "build_daemon", "assemble_runtime", "assemble_vscode", "check_protocol_contract"):
                stack.enter_context(action(name))
            stack.enter_context(patch.object(build, "build_workspace_ui"))
            stack.enter_context(patch.object(build.shutil, "rmtree"))
            tasks.main(["build", "vscode"])
        browser.assert_called_once()
        entries = [arg for call in calls.call_args_list for arg in call.args if arg.endswith(".mts")]
        self.assertIn("adapters/vscode/webview/vscode-host.mts", entries)
        self.assertNotIn("web/preview/browser-host.mts", entries)
        watches = " ".join(arg for command in build.watch_commands() for arg in command)
        self.assertIn("web/preview/browser-host.mts", watches)
        self.assertIn("adapters/vscode/webview/vscode-host.mts", watches)

    def test_quality_covers_owned_workspaces_and_fresh_generated_sources(self):
        with ExitStack() as stack:
            run = stack.enter_context(action("run"))
            yarn = stack.enter_context(action("yarn"))
            for name in ("generate_versions", "check_protocol_contract", "python_test", "check_localization", "pure_test"):
                stack.enter_context(action(name))
            tasks.main(["ci", "quality"])
        commands = [call.args for call in run.call_args_list]
        for manifest in ("adapters/zed/Cargo.toml",):
            for gate in ("fmt", "test", "clippy"):
                self.assertTrue(any(command[:2] == ("cargo", gate) and manifest in command for command in commands))
        for command in commands:
            if len(command) > 1 and command[0] == "cargo" and command[1] not in {"fmt", "xtask"}:
                self.assertIn("--locked", command)
        for gate in ("fmt", "test", "clippy", "xtask"):
            self.assertTrue(any(command[:2] == ("cargo", gate) for command in commands))
        self.assertNotIn(("npm", "audit"), [call.args[:2] for call in yarn.call_args_list])
        self.assertNotIn(("yarn", "npm", "audit"), [command[:3] for command in commands])

    def test_audit_is_an_explicit_network_gate(self):
        with action("run") as run:
            task_checks.verify_audit(())
        run.assert_called_once_with("yarn", "npm", "audit", "--all", "--recursive", "--no-deprecations", "--severity", "moderate")

    def test_generated_freshness_accepts_current_uncommitted_content(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            file = root / "generated.txt"
            file.write_text("current uncommitted output", encoding="utf-8")
            task_checks.check_generated_freshness(lambda: file.write_text("current uncommitted output", encoding="utf-8"), [root])

    def test_generated_freshness_rejects_changed_added_and_deleted_files(self):
        for mutation in ("changed", "added", "deleted"):
            with self.subTest(mutation=mutation), tempfile.TemporaryDirectory() as temporary:
                root = Path(temporary)
                file = root / "generated.txt"
                if mutation != "added":
                    file.write_text("old", encoding="utf-8")
                def generate():
                    if mutation == "deleted":
                        file.unlink()
                    else:
                        file.write_text("new", encoding="utf-8")
                with self.assertRaisesRegex(RuntimeError, "generated.txt"):
                    task_checks.check_generated_freshness(generate, [root])

    def test_local_verify_builds_shared_runtime_once_and_verifies_both_editors(self):
        with ExitStack() as stack:
            stack.enter_context(action("check_steps", return_value=[]))
            stack.enter_context(action("unit_steps", return_value=[]))
            gates = {name: stack.enter_context(action(name)) for name in (
                "verify_audit", "check_protocol_contract", "generate_versions", "build_daemon",
                "assemble_runtime", "build_vscode_product", "build_zed_product", "verify_built_product",
            )}
            browser = stack.enter_context(patch.object(javascript_build, "build_browser_client"))
            workspace = stack.enter_context(patch.object(javascript_build, "build_workspace_ui"))
            stack.enter_context(patch.dict(tasks.os.environ))
            tasks.main(["verify"])
        for name, gate in gates.items():
            if name == "check_protocol_contract":
                gate.assert_not_called()
            else:
                gate.assert_called_once()
        browser.assert_called_once_with()
        workspace.assert_called_once_with()

    def test_package_identity_is_strict_for_explicit_and_current_source_heads(self):
        for explicit in (True, False):
            with self.subTest(explicit=explicit), ExitStack() as stack:
                head = "a" * 40
                package = stack.enter_context(action("package_built_product", return_value=Path("product.vsix")))
                create = stack.enter_context(patch("release_artifact.create_identity"))
                verify = stack.enter_context(patch("release_artifact.verify_identity"))
                git = stack.enter_context(patch.object(task_products.subprocess, "check_output", return_value=head + "\n"))
                stack.enter_context(patch.object(task_products, "executable", return_value="git-tool"))
                stack.enter_context(patch.object(task_products, "metadata", return_value={"version": "1.2.3"}))
                stack.enter_context(patch.dict(task_products.os.environ, {"SOURCE_GIT_HEAD": head if explicit else ""}))
                self.assertEqual(task_products.package_identified_product(("--allow-missing-repository",)), Path("product.vsix"))
                package.assert_called_once_with(("--allow-missing-repository",))
                create.assert_called_once_with(Path("product.vsix"), Path("product.vsix.identity.json"), expected_version="1.2.3", git_tag="v1.2.3", source_git_head=head)
                verify.assert_called_once_with(Path("product.vsix"), Path("product.vsix.identity.json"), expected_git_tag="v1.2.3", expected_source_git_head=head)
                if explicit:
                    git.assert_not_called()
                else:
                    git.assert_called_once_with(["git-tool", "rev-parse", "HEAD"], cwd=task_products.ROOT, text=True)

    def test_identity_failures_propagate_without_running_later_gates(self):
        for failure in ("package", "create", "verify"):
            with self.subTest(failure=failure), ExitStack() as stack:
                package = stack.enter_context(action("package_built_product", return_value=Path("product.vsix")))
                create = stack.enter_context(patch("release_artifact.create_identity"))
                verify = stack.enter_context(patch("release_artifact.verify_identity"))
                smoke = stack.enter_context(action("smoke_vsix"))
                stack.enter_context(patch.dict(task_products.os.environ, {"SOURCE_GIT_HEAD": "a" * 40}))
                {"package": package, "create": create, "verify": verify}[failure].side_effect = RuntimeError("identity gate failed")
                with self.assertRaisesRegex(RuntimeError, "identity gate failed"):
                    task_products.package_identified_product()
                if failure == "package":
                    create.assert_not_called()
                if failure != "verify":
                    verify.assert_not_called()
                smoke.assert_not_called()

    def test_product_gate_runs_performance_editor_identity_and_clean_install(self):
        order = []
        with ExitStack() as stack:
            gates = {name: stack.enter_context(action(name)) for name in ("check_performance_budgets", "compile_tests", "run_editor_integration", "smoke_vsix")}
            package = stack.enter_context(action("package_built_product", return_value=Path("product.vsix")))
            create = stack.enter_context(patch("release_artifact.create_identity"))
            verify = stack.enter_context(patch("release_artifact.verify_identity"))
            stack.enter_context(patch.dict(tasks.os.environ, {"SOURCE_GIT_HEAD": "a" * 40}))
            for name, gate in gates.items():
                gate.side_effect = lambda *args, name=name: order.append(name)
            package.side_effect = lambda *args: order.append("package") or Path("product.vsix")
            create.side_effect = lambda *args, **kwargs: order.append("create")
            verify.side_effect = lambda *args, **kwargs: order.append("verify")
            task_products.verify_built_product()
        self.assertEqual(order, ["check_performance_budgets", "compile_tests", "run_editor_integration", "package", "create", "verify", "smoke_vsix"])
        for gate in gates.values():
            gate.assert_called_once()
        package.assert_called_once_with(())
        create.assert_called_once()
        verify.assert_called_once()
        self.assertEqual(verify.call_args.kwargs["expected_source_git_head"], "a" * 40)
        gates["smoke_vsix"].assert_called_once_with(Path("product.vsix"))


if __name__ == "__main__":
    unittest.main()
