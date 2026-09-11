from __future__ import annotations

import hashlib
import json
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch


SCRIPTS = Path(__file__).resolve().parents[1]
if str(SCRIPTS) not in sys.path:
    sys.path.insert(0, str(SCRIPTS))

import _tools
import create_release_manifest
import smoke_vsix
import stage_daemon


class PlatformMappingTests(unittest.TestCase):
    def test_maps_supported_operating_systems(self) -> None:
        for reported, expected in (
            ("win32", "win32"),
            ("darwin", "darwin"),
            ("linux", "linux"),
            ("linux-musl", "linux"),
        ):
            with self.subTest(reported=reported), patch.object(
                stage_daemon.sys, "platform", reported
            ):
                self.assertEqual(stage_daemon.platform_name(), expected)

    def test_maps_supported_architectures(self) -> None:
        for reported, expected in (
            ("AMD64", "x64"),
            ("x86_64", "x64"),
            ("ARM64", "arm64"),
            ("aarch64", "arm64"),
        ):
            with self.subTest(reported=reported), patch.object(
                stage_daemon.platform, "machine", return_value=reported
            ):
                self.assertEqual(stage_daemon.architecture_name(), expected)

    def test_rejects_unsupported_platform_and_architecture(self) -> None:
        with patch.object(stage_daemon.sys, "platform", "freebsd"):
            with self.assertRaisesRegex(
                RuntimeError, "unsupported platform: freebsd"
            ):
                stage_daemon.platform_name()
        with patch.object(stage_daemon.platform, "machine", return_value="riscv64"):
            with self.assertRaisesRegex(
                RuntimeError, "unsupported architecture: riscv64"
            ):
                stage_daemon.architecture_name()

    def test_smoke_workflow_uses_the_same_supported_platform_mapping(self) -> None:
        for reported, expected in (
            ("win32", "win32"),
            ("darwin", "darwin"),
            ("linux", "linux"),
            ("linux-musl", "linux"),
        ):
            with self.subTest(reported=reported), patch.object(
                smoke_vsix.sys, "platform", reported
            ):
                self.assertEqual(smoke_vsix.platform_name(), expected)

        for reported, expected in (
            ("AMD64", "x64"),
            ("x86_64", "x64"),
            ("ARM64", "arm64"),
            ("aarch64", "arm64"),
        ):
            with self.subTest(reported=reported), patch.object(
                smoke_vsix.platform, "machine", return_value=reported
            ):
                self.assertEqual(smoke_vsix.architecture_name(), expected)

        with patch.object(smoke_vsix.sys, "platform", "freebsd"):
            with self.assertRaisesRegex(
                RuntimeError, "unsupported platform: freebsd"
            ):
                smoke_vsix.platform_name()
        with patch.object(smoke_vsix.platform, "machine", return_value="riscv64"):
            with self.assertRaisesRegex(
                RuntimeError, "unsupported architecture: riscv64"
            ):
                smoke_vsix.architecture_name()


class StageDaemonTests(unittest.TestCase):
    def test_stages_unix_daemon_in_platform_architecture_directory(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            source = root / "target" / "release" / "fleximarkd"
            source.parent.mkdir(parents=True)
            source.write_bytes(b"daemon-binary")

            with (
                patch.object(stage_daemon, "ROOT", root),
                patch.object(stage_daemon.sys, "platform", "linux"),
                patch.object(
                    stage_daemon.platform, "machine", return_value="aarch64"
                ),
                patch.object(Path, "chmod") as chmod,
            ):
                stage_daemon.stage_daemon()

            destination = root / "bin" / "linux-arm64" / "fleximarkd"
            self.assertEqual(destination.read_bytes(), b"daemon-binary")
            chmod.assert_called_once_with(0o755)


class ReleaseManifestTests(unittest.TestCase):
    def test_writes_partial_manifest_with_stable_order_and_content_hashes(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            payloads = {
                "bin/linux-x64/fleximarkd": b"linux daemon",
                "bin/win32-arm64/fleximarkd.exe": b"windows daemon",
            }
            for relative_path, payload in payloads.items():
                artifact = root / relative_path
                artifact.parent.mkdir(parents=True, exist_ok=True)
                artifact.write_bytes(payload)

            with patch.object(create_release_manifest, "ROOT", root):
                create_release_manifest.create_manifest()

            output = root / "bin" / "manifest.json"
            self.assertTrue(output.read_bytes().endswith(b"\n"))
            self.assertEqual(
                json.loads(output.read_text(encoding="utf-8")),
                {
                    "schemaVersion": 1,
                    "protocolVersion": 1,
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
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            artifact = root / "bin" / "linux-x64" / "fleximarkd"
            artifact.parent.mkdir(parents=True)
            artifact.write_bytes(b"partial")

            with patch.object(create_release_manifest, "ROOT", root):
                with self.assertRaisesRegex(
                    RuntimeError,
                    "release requires Windows, macOS, and Linux daemons",
                ):
                    create_release_manifest.create_manifest(require_all=True)

            self.assertFalse((root / "bin" / "manifest.json").exists())


class ToolProcessTests(unittest.TestCase):
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


if __name__ == "__main__":
    unittest.main()
