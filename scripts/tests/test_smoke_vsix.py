from __future__ import annotations

import io
import json
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from unittest.mock import MagicMock, patch


SCRIPTS = Path(__file__).resolve().parents[1]
if str(SCRIPTS) not in sys.path:
    sys.path.insert(0, str(SCRIPTS))

import smoke_vsix


def frame(value: object, *, header_name: str = "Content-Length") -> bytes:
    body = json.dumps(value, separators=(",", ":")).encode()
    return f"{header_name}: {len(body)}\r\n\r\n".encode() + body


class RpcFrameTests(unittest.TestCase):
    def test_reads_rpc_response_using_case_insensitive_content_length(self) -> None:
        response = {"jsonrpc": "2.0", "id": 1, "result": {"ok": True}}
        self.assertEqual(
            smoke_vsix.read_rpc_message(
                io.BytesIO(frame(response, header_name="content-length"))
            ),
            response,
        )

    def test_rejects_missing_length_truncated_body_and_oversized_header(self) -> None:
        cases = (
            (
                io.BytesIO(b"Content-Type: application/json\r\n\r\n{}"),
                "packaged daemon response has no valid Content-Length",
            ),
            (
                io.BytesIO(b"Content-Length: 4\r\n\r\n{}"),
                "packaged daemon returned a truncated RPC response",
            ),
            (
                io.BytesIO(b"X: " + b"a" * (64 * 1024) + b"\r\n\r\n"),
                "packaged daemon returned an oversized RPC header",
            ),
        )
        for stream, message in cases:
            with self.subTest(message=message):
                with self.assertRaisesRegex(RuntimeError, message):
                    smoke_vsix.read_rpc_message(stream)

    def test_initialize_cleans_up_when_request_write_fails(self) -> None:
        child = MagicMock()
        child.stdin.write.side_effect = BrokenPipeError("daemon closed")
        child.poll.return_value = None

        with patch.object(smoke_vsix.subprocess, "Popen", return_value=child):
            with self.assertRaisesRegex(BrokenPipeError, "daemon closed"):
                smoke_vsix.initialize_daemon(Path("fleximarkd"), 1)

        child.stdin.close.assert_called_once_with()
        child.kill.assert_called_once_with()
        child.wait.assert_called_once_with(timeout=5)


class VsixChecksumTests(unittest.TestCase):
    def test_rejects_daemon_when_manifest_checksum_does_not_match(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            vsix = root / "fleximark.vsix"
            vsix.write_bytes(b"vsix")
            executable = root / "code.exe"
            executable.write_bytes(b"code")
            cli = root / "code.cmd"
            cli.write_bytes(b"cli")

            def checked_vscode(
                _executable_path: Path, *args: str
            ) -> subprocess.CompletedProcess[str]:
                extensions = Path(args[args.index("--extensions-dir") + 1])
                if "--install-extension" in args:
                    install_root = (
                        extensions / "kashiwade.fleximark-0.16.14" / "bin"
                    )
                    daemon = install_root / "linux-x64" / "fleximarkd"
                    daemon.parent.mkdir(parents=True)
                    daemon.write_bytes(b"packaged-daemon")
                    (install_root / "manifest.json").write_text(
                        json.dumps(
                            {
                                "schemaVersion": 1,
                                "protocolVersion": 2,
                                "artifacts": [
                                    {
                                        "platform": "linux",
                                        "arch": "x64",
                                        "path": "bin/linux-x64/fleximarkd",
                                        "sha256": "0" * 64,
                                    }
                                ],
                            }
                        ),
                        encoding="utf-8",
                    )
                    stdout = ""
                else:
                    stdout = "kashiwade.fleximark\n"
                return subprocess.CompletedProcess(args, 0, stdout, "")

            with (
                patch.object(smoke_vsix, "vscode_executable", return_value=executable),
                patch.object(smoke_vsix, "vscode_cli", return_value=cli),
                patch.object(smoke_vsix, "checked_vscode", side_effect=checked_vscode),
                patch.object(smoke_vsix, "platform_name", return_value="linux"),
                patch.object(smoke_vsix, "architecture_name", return_value="x64"),
                patch.object(smoke_vsix, "initialize_daemon") as initialize,
            ):
                with self.assertRaisesRegex(
                    RuntimeError,
                    "packaged daemon checksum does not match the manifest",
                ):
                    smoke_vsix.smoke_vsix(vsix)

            initialize.assert_not_called()


if __name__ == "__main__":
    unittest.main()
