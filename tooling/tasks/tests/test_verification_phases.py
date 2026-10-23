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


class VerificationPhasesTests(unittest.TestCase):
    def test_runtime_browser_is_built_once_and_extension_only_builds_its_host(self):
        with ExitStack() as stack:
            browser = stack.enter_context(patch.object(build, "build_browser_client"))
            calls = stack.enter_context(patch.object(build, "yarn"))
            for name in ("generate_versions", "build_daemon", "assemble_runtime", "assemble_vscode"):
                stack.enter_context(patch.object(tasks, name))
            stack.enter_context(patch.object(build, "build_workspace_ui"))
            stack.enter_context(patch.object(build.shutil, "rmtree"))
            tasks.build_vscode_product(tasks.build_runtime())
        browser.assert_called_once()
        entries = [arg for call in calls.call_args_list for arg in call.args if arg.endswith(".mts")]
        self.assertIn("adapters/vscode/webview/vscode-host.mts", entries)
        self.assertNotIn("web/preview/browser-host.mts", entries)
        watches = " ".join(arg for command in build.watch_commands() for arg in command)
        self.assertIn("web/preview/browser-host.mts", watches)
        self.assertIn("adapters/vscode/webview/vscode-host.mts", watches)

    def test_quality_covers_owned_workspaces_and_fresh_generated_sources(self):
        with ExitStack() as stack:
            run = stack.enter_context(patch.object(tasks, "run"))
            yarn = stack.enter_context(patch.object(tasks, "yarn"))
            for name in ("generate_versions", "check_protocol_contract", "python_test", "check_localization", "pure_test"):
                stack.enter_context(patch.object(tasks, name))
            tasks.main(["ci", "quality"])
        commands = [call.args for call in run.call_args_list]
        for manifest in ("adapters/zed/Cargo.toml", "tests/fixtures/plugin-component/Cargo.toml"):
            for gate in ("fmt", "test", "clippy"):
                self.assertTrue(any(command[:2] == ("cargo", gate) and manifest in command for command in commands))
        for gate in ("fmt", "test", "clippy", "xtask"):
            self.assertTrue(any(command[:2] == ("cargo", gate) for command in commands))
        self.assertTrue(any("wasm32-wasip2" in command for command in commands))
        self.assertNotIn(("npm", "audit", "--all", "--recursive", "--severity", "high"), [call.args for call in yarn.call_args_list])

    def test_audit_is_an_explicit_network_gate(self):
        with patch.object(tasks, "run") as run:
            tasks.verify_audit(())
        run.assert_called_once_with("yarn", "npm", "audit", "--all", "--recursive", "--severity", "high")

    def test_generated_freshness_accepts_current_uncommitted_content(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            file = root / "generated.txt"
            file.write_text("current uncommitted output", encoding="utf-8")
            tasks.check_generated_freshness(lambda: file.write_text("current uncommitted output", encoding="utf-8"), [root])

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
                    tasks.check_generated_freshness(generate, [root])

    def test_local_verify_builds_shared_runtime_once_and_verifies_both_editors(self):
        with ExitStack() as stack:
            stack.enter_context(patch.object(tasks, "check_steps", return_value=[]))
            stack.enter_context(patch.object(tasks, "unit_steps", return_value=[]))
            gates = {name: stack.enter_context(patch.object(tasks, name)) for name in (
                "verify_audit", "check_protocol_contract", "generate_versions", "build_daemon",
                "assemble_runtime", "build_vscode_product", "build_zed_product", "verify_built_product",
            )}
            browser = stack.enter_context(patch.object(tasks.javascript_build, "build_browser_client"))
            workspace = stack.enter_context(patch.object(tasks.javascript_build, "build_workspace_ui"))
            stack.enter_context(patch.dict(tasks.os.environ))
            tasks.main(["verify"])
        for gate in gates.values():
            gate.assert_called_once()
        browser.assert_called_once_with()
        workspace.assert_called_once_with()

    def test_product_gate_runs_performance_editor_identity_and_clean_install(self):
        with ExitStack() as stack:
            gates = {name: stack.enter_context(patch.object(tasks, name)) for name in ("check_performance_budgets", "compile_tests", "run_editor_integration", "smoke_vsix")}
            package = stack.enter_context(patch.object(tasks, "package_built_product", return_value=Path("product.vsix")))
            create = stack.enter_context(patch("release_artifact.create_identity"))
            verify = stack.enter_context(patch("release_artifact.verify_identity"))
            stack.enter_context(patch.dict(tasks.os.environ, {"SOURCE_GIT_HEAD": "a" * 40}))
            tasks.verify_built_product()
        for gate in gates.values():
            gate.assert_called_once()
        package.assert_called_once_with(())
        create.assert_called_once()
        verify.assert_called_once()
        self.assertEqual(verify.call_args.kwargs["expected_source_git_head"], "a" * 40)
        gates["smoke_vsix"].assert_called_once_with(Path("product.vsix"))


if __name__ == "__main__":
    unittest.main()
