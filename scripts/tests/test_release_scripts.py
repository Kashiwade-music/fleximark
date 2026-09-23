from __future__ import annotations

from contextlib import redirect_stderr
from dataclasses import FrozenInstanceError
import hashlib
import io
import json
import os
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from unittest.mock import MagicMock, call, patch


SCRIPTS = Path(__file__).resolve().parents[1]
if str(SCRIPTS) not in sys.path:
    sys.path.insert(0, str(SCRIPTS))

import _tools
import _targets
import create_release_manifest
import stage_daemon
import tasks


class TemporaryReleaseRootTests(unittest.TestCase):
    def setUp(self) -> None:
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)

    def write_bytes(self, relative_path: str, content: bytes) -> Path:
        path = self.root / relative_path
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(content)
        return path


class PlatformMappingTests(unittest.TestCase):
    def test_target_owner_matches_workflow_matrices(self) -> None:
        expected = [
            ("ubuntu-24.04", "linux", "x64"),
            ("ubuntu-24.04-arm", "linux", "arm64"),
            ("macos-15-intel", "darwin", "x64"),
            ("macos-15", "darwin", "arm64"),
            ("windows-2025", "win32", "x64"),
            ("windows-11-arm", "win32", "arm64"),
        ]
        self.assertEqual(
            [(target.platform, target.arch) for target in _targets.TARGETS],
            [(platform_name, arch) for _, platform_name, arch in expected],
        )
        with self.assertRaises(FrozenInstanceError):
            _targets.TARGETS[0].arch = "arm64"  # type: ignore[misc]

        for workflow in ("ci.yml", "release.yml"):
            source = (_tools.ROOT / ".github" / "workflows" / workflow).read_text(
                encoding="utf-8"
            )
            for os_name, platform_name, arch in expected:
                row = f"{{ os: {os_name}, platform: {platform_name}, arch: {arch} }}"
                self.assertEqual(source.count(row), 2, (workflow, row))

    def test_normalizes_supported_platforms_and_architectures(self) -> None:
        for normalizer, cases in (
            (
                _targets.normalize_platform,
                (("win32", "win32"), ("darwin", "darwin"), ("linux-musl", "linux")),
            ),
            (
                _targets.normalize_arch,
                (
                    ("AMD64", "x64"),
                    ("x86_64", "x64"),
                    ("ARM64", "arm64"),
                    ("aarch64", "arm64"),
                ),
            ),
        ):
            for reported, expected in cases:
                with self.subTest(reported=reported):
                    self.assertEqual(normalizer(reported), expected)

    def test_rejects_unsupported_platform_and_architecture(self) -> None:
        with self.assertRaisesRegex(RuntimeError, "unsupported platform: freebsd"):
            _targets.normalize_platform("freebsd")
        with self.assertRaisesRegex(RuntimeError, "unsupported architecture: riscv64"):
            _targets.normalize_arch("riscv64")


class StageDaemonTests(TemporaryReleaseRootTests):
    def test_stages_unix_daemon_in_platform_architecture_directory(self) -> None:
        self.write_bytes("target/release/fleximarkd", b"daemon-binary")
        with (
            patch.object(stage_daemon, "ROOT", self.root),
            patch.object(stage_daemon.sys, "platform", "linux"),
            patch.object(stage_daemon.platform, "machine", return_value="aarch64"),
            patch.object(Path, "chmod") as chmod,
        ):
            stage_daemon.stage_daemon()

        destination = self.root / "bin" / "linux-arm64" / "fleximarkd"
        self.assertEqual(destination.read_bytes(), b"daemon-binary")
        chmod.assert_called_once_with(0o755)

    def test_stages_windows_daemon_with_exe_suffix(self) -> None:
        self.write_bytes("target/release/fleximarkd.exe", b"windows-daemon")
        with (
            patch.object(stage_daemon, "ROOT", self.root),
            patch.object(stage_daemon.sys, "platform", "win32"),
            patch.object(stage_daemon.platform, "machine", return_value="AMD64"),
            patch.object(Path, "chmod") as chmod,
        ):
            stage_daemon.stage_daemon()

        destination = self.root / "bin" / "win32-x64" / "fleximarkd.exe"
        self.assertEqual(destination.read_bytes(), b"windows-daemon")
        chmod.assert_not_called()


class ReleaseManifestTests(TemporaryReleaseRootTests):
    @unittest.skipUnless(os.name == "posix", "POSIX mode bits are required")
    def test_manifest_normalizes_downloaded_unix_daemon_mode(self) -> None:
        artifact = self.write_bytes("bin/linux-x64/fleximarkd", b"downloaded daemon")
        artifact.chmod(0o644)
        with patch.object(create_release_manifest, "ROOT", self.root):
            create_release_manifest.create_manifest()
        self.assertEqual(artifact.stat().st_mode & 0o777, 0o755)

    def test_manifest_does_not_chmod_downloaded_windows_daemon(self) -> None:
        self.write_bytes("bin/win32-x64/fleximarkd.exe", b"downloaded windows daemon")
        with (
            patch.object(create_release_manifest, "ROOT", self.root),
            patch.object(Path, "chmod") as chmod,
        ):
            create_release_manifest.create_manifest()
        chmod.assert_not_called()

    def test_require_all_emits_exactly_the_six_supported_daemon_targets(self) -> None:
        expected = [
            ("linux", "x64", "bin/linux-x64/fleximarkd"),
            ("linux", "arm64", "bin/linux-arm64/fleximarkd"),
            ("darwin", "x64", "bin/darwin-x64/fleximarkd"),
            ("darwin", "arm64", "bin/darwin-arm64/fleximarkd"),
            ("win32", "x64", "bin/win32-x64/fleximarkd.exe"),
            ("win32", "arm64", "bin/win32-arm64/fleximarkd.exe"),
        ]
        for _, _, relative_path in expected:
            self.write_bytes(relative_path, relative_path.encode())
        with patch.object(create_release_manifest, "ROOT", self.root):
            create_release_manifest.create_manifest(require_all=True)

        manifest = json.loads(
            (self.root / "bin" / "manifest.json").read_text(encoding="utf-8")
        )
        self.assertEqual(
            [
                (item["platform"], item["arch"], item["path"])
                for item in manifest["artifacts"]
            ],
            expected,
        )

    def test_writes_partial_manifest_with_stable_order_and_content_hashes(self) -> None:
        payloads = {
            "bin/linux-x64/fleximarkd": b"linux daemon",
            "bin/win32-arm64/fleximarkd.exe": b"windows daemon",
        }
        for relative_path, payload in payloads.items():
            self.write_bytes(relative_path, payload)
        with patch.object(create_release_manifest, "ROOT", self.root):
            create_release_manifest.create_manifest()

        output = self.root / "bin" / "manifest.json"
        self.assertTrue(output.read_bytes().endswith(b"\n"))
        self.assertEqual(
            json.loads(output.read_text(encoding="utf-8")),
            {
                "schemaVersion": 1,
                "protocolVersion": 4,
                "artifacts": [
                    {
                        "platform": "linux",
                        "arch": "x64",
                        "path": "bin/linux-x64/fleximarkd",
                        "sha256": hashlib.sha256(
                            payloads["bin/linux-x64/fleximarkd"]
                        ).hexdigest(),
                    },
                    {
                        "platform": "win32",
                        "arch": "arm64",
                        "path": "bin/win32-arm64/fleximarkd.exe",
                        "sha256": hashlib.sha256(
                            payloads["bin/win32-arm64/fleximarkd.exe"]
                        ).hexdigest(),
                    },
                ],
            },
        )

    def test_require_all_rejects_a_partial_release_without_writing_manifest(self) -> None:
        self.write_bytes("bin/linux-x64/fleximarkd", b"partial")
        with patch.object(create_release_manifest, "ROOT", self.root):
            with self.assertRaisesRegex(
                RuntimeError,
                "release requires Windows, macOS, and Linux daemons",
            ):
                create_release_manifest.create_manifest(require_all=True)
        self.assertFalse((self.root / "bin" / "manifest.json").exists())


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


class TaskEntryPointTests(unittest.TestCase):
    def test_generic_vscode_prepublish_keeps_the_complete_build(self) -> None:
        with (
            patch.dict(os.environ, {}, clear=True),
            patch.object(tasks, "build") as build,
        ):
            tasks.vscode_prepublish(())

        build.assert_called_once_with(())

    def test_release_vscode_prepublish_preserves_downloaded_prebuilt_inputs(
        self,
    ) -> None:
        with (
            patch.dict(os.environ, {"FLEXIMARK_RELEASE_PREBUILT": "1"}),
            patch.object(tasks, "build") as build,
            patch.object(tasks, "validate_prebuilt_inputs") as validate,
        ):
            tasks.vscode_prepublish(())

        build.assert_not_called()
        validate.assert_called_once_with(tasks.ROOT)

    def test_standalone_test_performs_the_complete_build_before_vscode(self) -> None:
        events: list[str] = []

        def record(name: str):
            return lambda *args, **kwargs: events.append(name)

        with (
            patch.object(tasks, "compile_tests", side_effect=record("tests")),
            patch.object(
                tasks.javascript_build,
                "build_browser_client",
                side_effect=record("browser"),
            ),
            patch.object(tasks, "run", side_effect=record("cargo")) as run,
            patch.object(tasks, "stage_daemon", side_effect=record("stage")),
            patch.object(tasks, "create_manifest", side_effect=record("manifest")),
            patch.object(
                tasks.javascript_build,
                "build_extension",
                side_effect=record("extension"),
            ),
            patch.object(tasks, "yarn", side_effect=record("vscode")) as yarn,
        ):
            tasks.TASKS["test"](())

        self.assertEqual(
            events,
            [
                "tests",
                "browser",
                "cargo",
                "stage",
                "manifest",
                "extension",
                "vscode",
            ],
        )
        run.assert_called_once_with(
            "cargo", "build", "--release", "-p", "fleximarkd", "--locked"
        )
        yarn.assert_called_once_with("vscode-test")

    def test_prebuilt_test_builds_only_tests_before_vscode(self) -> None:
        events: list[str] = []
        with (
            patch.object(
                tasks, "compile_tests", side_effect=lambda: events.append("tests")
            ),
            patch.object(tasks, "build") as build,
            patch.object(
                tasks, "yarn", side_effect=lambda *args: events.append("vscode")
            ) as yarn,
        ):
            tasks.integration_test(("--prebuilt",))

        self.assertEqual(events, ["tests", "vscode"])
        build.assert_not_called()
        yarn.assert_called_once_with("vscode-test")

    def test_package_keeps_dependency_free_vscode_packaging(self) -> None:
        with (
            patch.object(tasks, "verify") as verify,
            patch.object(tasks, "yarn") as yarn,
        ):
            tasks.package_vsix(("--out", "artifact.vsix"))

        verify.assert_called_once_with(())
        yarn.assert_called_once_with(
            "vsce", "package", "--no-dependencies", "--out", "artifact.vsix"
        )

    def test_runtime_error_at_cli_boundary_prints_one_error_and_exits_one(self) -> None:
        result = subprocess.run(
            [sys.executable, str(SCRIPTS / "tasks.py"), "smoke"],
            cwd=SCRIPTS.parent,
            capture_output=True,
            encoding="utf-8",
            errors="replace",
            check=False,
        )

        self.assertEqual(result.returncode, 1)
        self.assertEqual(result.stdout, "")
        self.assertEqual(
            result.stderr.strip(), "error: smoke requires exactly one VSIX path"
        )


class WatchCleanupTests(unittest.TestCase):
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
            patch.object(tasks, "build_daemon") as build_daemon,
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
            tasks.dev(())

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
                "FlexiMark development build starting",
                "sleep",
                "FlexiMark development build ready",
                "sleep",
            ],
        )
        stop.assert_called_once_with(children)
        build_daemon.assert_called_once_with()

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
            patch.object(tasks, "build_daemon"),
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
                tasks.dev(())

        child.terminate.assert_called_once_with()
        child.wait.assert_called_once_with(timeout=5)

if __name__ == "__main__":
    unittest.main()
