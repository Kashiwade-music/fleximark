from __future__ import annotations

import platform
import shutil
import sys

from _tools import ROOT


def platform_name() -> str:
    if sys.platform == "win32":
        return "win32"
    if sys.platform == "darwin":
        return "darwin"
    if sys.platform.startswith("linux"):
        return "linux"
    raise RuntimeError(f"unsupported platform: {sys.platform}")


def architecture_name() -> str:
    machine = platform.machine().lower()
    if machine in {"amd64", "x86_64"}:
        return "x64"
    if machine in {"arm64", "aarch64"}:
        return "arm64"
    raise RuntimeError(f"unsupported architecture: {machine}")


def stage_daemon() -> None:
    executable = "fleximarkd.exe" if sys.platform == "win32" else "fleximarkd"
    source = ROOT / "target" / "release" / executable
    destination = (
        ROOT
        / "bin"
        / f"{platform_name()}-{architecture_name()}"
        / executable
    )
    destination.parent.mkdir(parents=True, exist_ok=True)
    shutil.copy2(source, destination)
    if sys.platform != "win32":
        destination.chmod(0o755)


if __name__ == "__main__":
    stage_daemon()
