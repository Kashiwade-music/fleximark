from __future__ import annotations

import argparse
import concurrent.futures
import hashlib
import json
import platform
import subprocess
import sys
import tempfile
from pathlib import Path
from typing import Any, BinaryIO

from _targets import find_target, normalize_arch, normalize_platform
from _tools import ROOT, run, script_entrypoint


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
        [str(daemon), "rpc"],
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
                "method": "fleximark/initialize",
                "params": {
                    "protocolVersion": protocol_version,
                    "client": {"name": "release-smoke", "version": "1"},
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

    result = response.get("result")
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
    code = vscode_cli(vscode_executable())

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
        manifest = json.loads(
            (extension_root / "bin" / "manifest.json").read_text(encoding="utf-8")
        )
        if manifest.get("schemaVersion") != 1:
            raise RuntimeError("unsupported release manifest")
        target = find_target(platform_name(), architecture_name())
        artifact = next(
            (
                item
                for item in manifest["artifacts"]
                if item["platform"] == target.platform
                and item["arch"] == target.arch
            ),
            None,
        )
        if artifact is None:
            raise RuntimeError("manifest has no daemon for this platform")

        daemon = (extension_root / Path(artifact["path"])).resolve()
        if not daemon.is_relative_to(extension_root):
            raise RuntimeError("manifest daemon path escapes the extension")
        checksum = hashlib.sha256(daemon.read_bytes()).hexdigest()
        if checksum != artifact["sha256"]:
            raise RuntimeError("packaged daemon checksum does not match the manifest")
        initialize_daemon(daemon, manifest["protocolVersion"])


def main() -> None:
    parser = argparse.ArgumentParser(description="Smoke-test an installed FlexiMark VSIX")
    parser.add_argument("vsix", nargs="?", default="fleximark.vsix", type=Path)
    args = parser.parse_args()
    smoke_vsix(args.vsix)


if __name__ == "__main__":
    script_entrypoint(main)
