from __future__ import annotations

import concurrent.futures
import hashlib
import json
import platform
import subprocess
import sys
import tempfile
from pathlib import Path
from typing import Any, BinaryIO

from distribution_contract import normalize_arch, normalize_platform
from _tools import ROOT, run


EXTENSION_ID = "kashiwade.fleximark"


def platform_name() -> str:
    return normalize_platform(sys.platform)


def architecture_name() -> str:
    return normalize_arch(platform.machine())


def vscode_executable() -> Path:
    # @vscode/test-electron owns the download/cache contract. Python owns the
    # smoke workflow and treats this small Node API call as a tool boundary.
    source = (
        "import { downloadAndUnzipVSCode } from '@vscode/test-electron';"
        "console.log('__FLEXIMARK_VSCODE__' + "
        "await downloadAndUnzipVSCode('stable'));"
    )
    result = run(
        "node",
        "--input-type=module",
        "--eval",
        source,
        capture_output=True,
        timeout=300,
    )
    marker = "__FLEXIMARK_VSCODE__"
    paths = [line.removeprefix(marker) for line in result.stdout.splitlines() if line.startswith(marker)]
    if len(paths) != 1:
        raise RuntimeError("VS Code downloader did not return an executable path")
    path = Path(paths[0])
    if not path.is_file():
        raise RuntimeError(f"VS Code executable was not downloaded: {path}")
    return path


def vscode_cli(executable_path: Path) -> Path:
    candidates = [
        executable_path.parent / "bin" / "code.cmd",
        executable_path.parent / "bin" / "code",
        executable_path.parent.parent / "Resources" / "app" / "bin" / "code",
    ]
    cli = next((candidate for candidate in candidates if candidate.is_file()), None)
    if cli is None:
        raise RuntimeError(f"VS Code CLI was not found beside: {executable_path}")
    return cli


def checked_vscode(executable_path: Path, *args: str) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        [str(executable_path), *args],
        cwd=ROOT,
        capture_output=True,
        encoding="utf-8",
        errors="replace",
        check=True,
    )


def read_rpc_message(stream: BinaryIO) -> dict[str, Any]:
    header = bytearray()
    while not header.endswith(b"\r\n\r\n"):
        byte = stream.read(1)
        if not byte:
            raise RuntimeError("packaged daemon closed before returning a response")
        header.extend(byte)
        if len(header) > 64 * 1024:
            raise RuntimeError("packaged daemon returned an oversized RPC header")

    content_length: int | None = None
    for line in header[:-4].decode("ascii").split("\r\n"):
        name, separator, value = line.partition(":")
        if separator and name.lower() == "content-length":
            content_length = int(value.strip())
            break
    if content_length is None or content_length < 0:
        raise RuntimeError("packaged daemon response has no valid Content-Length")
    body = stream.read(content_length)
    if len(body) != content_length:
        raise RuntimeError("packaged daemon returned a truncated RPC response")
    return json.loads(body)


def initialize_daemon(daemon: Path, protocol_version: int) -> None:
    child = subprocess.Popen(
        [str(daemon), "lsp"],
        cwd=ROOT,
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        creationflags=(subprocess.CREATE_NO_WINDOW if sys.platform == "win32" else 0),
    )
    try:
        assert child.stdin is not None
        assert child.stdout is not None
        assert child.stderr is not None
        request = json.dumps(
            {
                "jsonrpc": "2.0",
                "id": 1,
                "method": "initialize",
                "params": {
                    "processId": None,
                    "rootUri": None,
                    "clientInfo": {"name": "release-smoke", "version": "1"},
                    "capabilities": {},
                },
            },
            separators=(",", ":"),
        ).encode()
        child.stdin.write(
            f"Content-Length: {len(request)}\r\n\r\n".encode() + request
        )
        child.stdin.flush()

        with concurrent.futures.ThreadPoolExecutor(max_workers=1) as pool:
            future = pool.submit(read_rpc_message, child.stdout)
            try:
                response = future.result(timeout=10)
            except TimeoutError as error:
                child.kill()
                details = child.stderr.read().decode(errors="replace")
                raise RuntimeError(
                    details or "packaged daemon initialize timed out"
                ) from error
    finally:
        if child.stdin is not None:
            try:
                child.stdin.close()
            except OSError:
                pass
        if child.poll() is None:
            child.kill()
        child.wait(timeout=5)

    result = response.get("result", {}).get("capabilities", {}).get("experimental", {}).get("fleximark")
    if (
        not isinstance(result, dict)
        or result.get("protocolVersion") != protocol_version
        or not isinstance(result.get("daemonInstanceId"), str)
    ):
        raise RuntimeError("packaged daemon protocol initialization failed")


def smoke_vsix(vsix: Path) -> None:
    vsix = vsix.resolve()
    if not vsix.is_file():
        raise RuntimeError(f"VSIX does not exist: {vsix}")
    executable_path = vscode_executable()
    code = vscode_cli(executable_path)

    with tempfile.TemporaryDirectory(prefix="fleximark-install-") as temp:
        scratch = Path(temp)
        extensions = scratch / "extensions"
        common = (
            "--extensions-dir",
            str(extensions),
            "--user-data-dir",
            str(scratch / "user-data"),
        )
        checked_vscode(code, *common, "--install-extension", str(vsix), "--force")
        installed = checked_vscode(code, *common, "--list-extensions")
        if EXTENSION_ID not in installed.stdout.lower():
            raise RuntimeError("installed extension is missing")

        candidates = [
            directory
            for directory in extensions.iterdir()
            if directory.is_dir() and directory.name.lower().startswith(f"{EXTENSION_ID}-")
        ]
        if not candidates:
            raise RuntimeError("installed extension files are missing")
        extension_root = candidates[0].resolve()
        from distribution import host_target, verify_runtime, metadata
        runtime = extension_root / "runtime"
        verify_runtime(runtime, host_target())
        suffix = ".exe" if sys.platform == "win32" else ""
        daemon = runtime / "bin" / ("fleximarkd" + suffix)
        initialize_daemon(daemon, metadata()["protocol_version"])
        run(
            "node", "tooling/tasks/smoke_webview.mjs",
            str(executable_path), str(scratch), str(extensions),
            timeout=120,
        )
