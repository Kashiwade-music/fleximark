from __future__ import annotations

from contextlib import redirect_stderr
import io
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from unittest.mock import MagicMock, call, patch

SCRIPTS = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(SCRIPTS))
import _tools
import tasks
import distribution
import build

class ToolProcessTests(unittest.TestCase):
    def test_windows_candidates_preserve_path_and_pathext_order(self) -> None:
        self.assertEqual(
            _tools.windows_executable_candidates(
                "sample",
                path="first|second",
                pathext=".CMD;.EXE",
                path_separator="|",
            ),
            (
                Path("first") / "sample.cmd",
                Path("first") / "sample.exe",
                Path("second") / "sample.cmd",
                Path("second") / "sample.exe",
            ),
        )

    def test_windows_candidates_use_default_and_skip_suffixed_names(self) -> None:
        defaults = _tools.windows_executable_candidates(
            "sample", path="tools", pathext=None, path_separator="|"
        )
        self.assertEqual(
            tuple(candidate.name for candidate in defaults),
            ("sample.com", "sample.exe", "sample.bat", "sample.cmd"),
        )
        self.assertEqual(
            _tools.windows_executable_candidates(
                "sample.exe",
                path="tools",
                pathext=".CMD;.EXE",
                path_separator="|",
            ),
            (),
        )

    def test_suffixed_executable_falls_back_to_which(self) -> None:
        checked: list[Path] = []
        which = MagicMock(return_value="resolved/sample.exe")

        def missing(candidate: Path) -> bool:
            checked.append(candidate)
            return False

        resolved = _tools.resolve_executable(
            "sample.exe",
            windows=True,
            path="first|second",
            pathext=".CMD;.EXE",
            path_separator="|",
            is_file=missing,
            which=which,
        )
        self.assertEqual(resolved, "resolved/sample.exe")
        self.assertEqual(checked, [])
        which.assert_called_once_with("sample.exe")

    def test_windows_candidate_misses_fall_back_to_which(self) -> None:
        checked: list[Path] = []
        which = MagicMock(return_value="resolved/sample")

        def missing(candidate: Path) -> bool:
            checked.append(candidate)
            return False

        resolved = _tools.resolve_executable(
            "sample",
            windows=True,
            path="first|second",
            pathext=".CMD;.EXE",
            path_separator="|",
            is_file=missing,
            which=which,
        )
        self.assertEqual(
            checked,
            [
                Path("first") / "sample.cmd",
                Path("first") / "sample.exe",
                Path("second") / "sample.cmd",
                Path("second") / "sample.exe",
            ],
        )
        self.assertEqual(resolved, "resolved/sample")
        which.assert_called_once_with("sample")

    def test_run_uses_repository_root_and_merges_environment_overrides(self) -> None:
        completed = subprocess.CompletedProcess(["tool", "arg"], 0, "ok", "")
        with (
            patch.object(_tools, "executable", return_value="resolved-tool"),
            patch.object(_tools.subprocess, "run", return_value=completed) as run,
        ):
            result = _tools.run(
                "tool",
                "arg",
                capture_output=True,
                env={"FLEXIMARK_TEST_VALUE": "present"},
                timeout=2.5,
            )

        self.assertIs(result, completed)
        run.assert_called_once()
        positional, keyword = run.call_args
        self.assertEqual(positional[0], ["resolved-tool", "arg"])
        self.assertEqual(keyword["cwd"], _tools.ROOT)
        self.assertTrue(keyword["check"])
        self.assertTrue(keyword["capture_output"])
        self.assertEqual(keyword["encoding"], "utf-8")
        self.assertEqual(keyword["errors"], "replace")
        self.assertEqual(keyword["timeout"], 2.5)
        self.assertEqual(keyword["env"]["FLEXIMARK_TEST_VALUE"], "present")

    def test_nonzero_process_preserves_exit_code_stdout_and_stderr(self) -> None:
        source = (
            "import sys; "
            "print('standard output', flush=True); "
            "print('standard error', file=sys.stderr, flush=True); "
            "raise SystemExit(7)"
        )
        with self.assertRaises(subprocess.CalledProcessError) as raised:
            _tools.run(sys.executable, "-c", source, capture_output=True)

        self.assertEqual(raised.exception.returncode, 7)
        self.assertEqual(raised.exception.stdout, "standard output\n")
        self.assertEqual(raised.exception.stderr, "standard error\n")

    def test_timed_out_process_preserves_partial_stdout_and_stderr(self) -> None:
        source = (
            "import sys, time; "
            "print('partial output', flush=True); "
            "print('partial error', file=sys.stderr, flush=True); "
            "time.sleep(10)"
        )
        with self.assertRaises(subprocess.TimeoutExpired) as raised:
            _tools.run(
                sys.executable,
                "-c",
                source,
                capture_output=True,
                timeout=0.25,
            )

        stdout = raised.exception.stdout
        stderr = raised.exception.stderr
        if isinstance(stdout, bytes):
            stdout = stdout.decode()
        if isinstance(stderr, bytes):
            stderr = stderr.decode()
        self.assertEqual(stdout, "partial output\n")
        self.assertEqual(stderr, "partial error\n")

    def test_entrypoint_formats_nonzero_and_timeout_with_binary_output(self) -> None:
        cases = (
            (
                subprocess.CalledProcessError(
                    7,
                    [b"tool", b"bad-\xff"],
                    output=b"standard-\xff-output\n",
                    stderr=b"standard-\xff-error\n",
                ),
                ("exit code 7", "tool", "stdout:", "standard-�-output", "stderr:", "standard-�-error"),
            ),
            (
                subprocess.TimeoutExpired(
                    [b"tool", b"slow"],
                    2.5,
                    output=b"partial-\xff-output\n",
                    stderr=b"partial-\xff-error\n",
                ),
                ("timed out after 2.5s", "tool slow", "stdout:", "partial-�-output", "stderr:", "partial-�-error"),
            ),
        )
        for error, expected_parts in cases:
            with self.subTest(error=type(error).__name__):
                stderr = io.StringIO()

                def fail() -> None:
                    raise error

                with redirect_stderr(stderr), self.assertRaisesRegex(SystemExit, "1"):
                    _tools.script_entrypoint(fail)
                for expected in expected_parts:
                    self.assertIn(expected, stderr.getvalue())


class DistributionTests(unittest.TestCase):
    def test_release_publish_requires_exact_tag_and_complete_product_set(self):
        import release_assets
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            with self.assertRaisesRegex(RuntimeError, "release tag differs"):
                release_assets.verify(root, "a" * 40, "wrong-tag")
            with self.assertRaisesRegex(RuntimeError, "exactly six"):
                release_assets.verify(root, "a" * 40, "v" + distribution.metadata()["version"])

    def test_native_build_has_no_node_or_browser_dependency(self):
        with patch.object(distribution, "run") as run:
            distribution.build_native()
            run.assert_called_once_with("cargo", "build", "--release", "--locked", "--bin", "fleximarkd", "--bin", "fleximark")

    def test_all_six_targets_have_canonical_runtime_and_hash_sidecar(self):
        import json
        import zipfile
        for target in distribution.TARGETS.values():
            with self.subTest(target=target), tempfile.TemporaryDirectory() as temp:
                root = Path(temp)
                (root / "release.toml").write_text('version = "1.2.3"\nprotocol_version = 5\n')
                native = root / "native"
                preview = root / "preview"
                workspace = root / ".build" / "workspace-ui"
                for directory in (native, preview, workspace):
                    directory.mkdir(parents=True)
                suffix = ".exe" if "windows" in target else ""
                for name in ("fleximarkd", "fleximark"):
                    (native / (name + suffix)).write_bytes(name.encode())
                for name in ("browser-host.js", "fleximark.css", "katex.css"):
                    (preview / name).write_bytes(name.encode())
                for name in ("workspace.js", "workspace.css"):
                    (workspace / name).write_bytes(name.encode())
                with patch.object(distribution, "ROOT", root):
                    archive = distribution.assemble_runtime(target, native_dir=native, preview_dir=preview)
                    distribution.verify_runtime(archive.parent / "runtime", target)
                    self.assertEqual(archive.with_suffix(".zip.sha256").read_text(), f"{distribution.digest(archive)}  {archive.name}\n")
                    with zipfile.ZipFile(archive) as stream:
                        manifest = json.loads(stream.read("manifest.json"))
                        self.assertEqual(set(stream.namelist()), set(manifest["files"]) | {"manifest.json"})
                        self.assertEqual(len(manifest["files"]), 7)
                    (archive.parent / "runtime" / "bin" / ("fleximark" + suffix)).write_bytes(b"tampered")
                    with self.assertRaisesRegex(RuntimeError, "checksum mismatch"):
                        distribution.verify_runtime(archive.parent / "runtime", target)

    def test_cleanup_refuses_paths_outside_artifact_staging(self):
        with self.assertRaisesRegex(RuntimeError, "outside artifact staging"):
            distribution.reset_output(distribution.ROOT / "core")

    def test_browser_and_adapter_have_distinct_entries_and_outputs(self):
        self.assertIn("adapters/vscode/webview/vscode-host.mts", build.preview_args(production=True))
        self.assertIn("--outfile=.build/vscode/dist/extension.cjs", build.extension_args(production=True))
        self.assertIn("tests/typescript/pure.test.mts", build.pure_test_args())

class WatchCleanupTests(unittest.TestCase):
    def setUp(self):
        for name in ("refresh_development_resources",):
            mock = patch.object(tasks, name)
            mock.start()
            self.addCleanup(mock.stop)
        copy = patch.object(tasks.shutil, "copytree")
        copy.start()
        self.addCleanup(copy.stop)
        check = patch.object(tasks, "check_protocol_contract")
        check.start()
        self.addCleanup(check.stop)

    def test_dev_announces_ready_only_after_every_watcher_is_ready(self) -> None:
        children = [MagicMock(), MagicMock(), MagicMock()]
        for child in children:
            child.poll.return_value = None
        ready_events = [MagicMock(), MagicMock(), MagicMock()]
        ready_events[0].is_set.return_value = True
        ready_events[1].is_set.return_value = True
        ready_events[2].is_set.side_effect = [False, True]
        events: list[str] = []

        def sleep(_: float) -> None:
            events.append("sleep")
            if events.count("sleep") == 2:
                raise KeyboardInterrupt

        def record_output(message: str, **_: object) -> None:
            events.append(message)

        with (
            patch.object(tasks.javascript_build, "clean"),
            patch.object(
                tasks.javascript_build,
                "watch_commands",
                return_value=[["watch-extension"], ["watch-preview"]],
            ),
            patch.object(
                tasks,
                "start_watch_process",
                side_effect=zip(children, ready_events, strict=True),
            ) as start,
            patch.object(tasks, "stop_processes") as stop,
            patch.object(tasks.time, "sleep", side_effect=sleep),
            patch("builtins.print", side_effect=record_output),
        ):
            tasks.watch_vscode()

        self.assertEqual(
            start.call_args_list,
            [
                call(["watch-extension"], "[watch] build finished"),
                call(["watch-preview"], "[watch] build finished"),
                call(
                    ["yarn", "exec", "tsc", "-b", "--watch"],
                    "Watching for file changes.",
                ),
            ],
        )
        self.assertEqual(
            events,
            [
                "FlexiMark VS Code watch starting",
                "sleep",
                "FlexiMark VS Code watch ready",
                "Watching JavaScript/TypeScript only. For Rust changes, stop the watch task and restart F5. Ctrl+C stops watching.",
                "sleep",
            ],
        )
        stop.assert_called_once_with(children)

    def test_stop_processes_terminates_then_waits_for_live_children(self) -> None:
        first = MagicMock()
        second = MagicMock()
        first.poll.side_effect = [None, None]
        second.poll.side_effect = [None, None]

        tasks.stop_processes([first, second])

        expected_calls = [
            call.poll(),
            call.terminate(),
            call.poll(),
            call.wait(timeout=5),
        ]
        first.assert_has_calls(expected_calls)
        second.assert_has_calls(expected_calls)
        first.kill.assert_not_called()
        second.kill.assert_not_called()

    def test_stop_processes_kills_a_child_that_ignores_termination(self) -> None:
        process = MagicMock()
        process.poll.side_effect = [None, None]
        process.wait.side_effect = [
            subprocess.TimeoutExpired(["watcher"], 5),
            None,
        ]

        tasks.stop_processes([process])

        process.terminate.assert_called_once_with()
        self.assertEqual(process.wait.call_args_list, [call(timeout=5), call(timeout=5)])
        process.kill.assert_called_once_with()

    def test_dev_cleans_up_started_children_when_a_later_spawn_fails(self) -> None:
        child = MagicMock()
        child.poll.side_effect = [None, None]
        with (
            patch.object(tasks, "check_protocol_contract"),
            patch.object(tasks.javascript_build, "clean"),
            patch.object(
                tasks.javascript_build,
                "watch_commands",
                return_value=[["watch-extension"], ["watch-preview"]],
            ),
            patch.object(tasks, "executable", side_effect=lambda name: name),
            patch.object(
                tasks.subprocess,
                "Popen",
                side_effect=[child, OSError("spawn failed")],
            ),
        ):
            with self.assertRaisesRegex(OSError, "spawn failed"):
                tasks.watch_vscode()

        child.terminate.assert_called_once_with()
        child.wait.assert_called_once_with(timeout=5)

if __name__ == "__main__":
    unittest.main()
