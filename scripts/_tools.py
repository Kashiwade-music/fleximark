from __future__ import annotations

import os
import shutil
import subprocess
from pathlib import Path


ROOT = Path(__file__).resolve().parent.parent


def executable(name: str) -> str:
    if os.name == "nt" and Path(name).suffix == "":
        extensions = os.environ.get("PATHEXT", ".COM;.EXE;.BAT;.CMD").split(";")
        for directory in os.environ.get("PATH", "").split(os.pathsep):
            for extension in extensions:
                candidate = Path(directory) / f"{name}{extension.lower()}"
                if candidate.is_file():
                    return str(candidate)
    resolved = shutil.which(name)
    if resolved is None:
        raise RuntimeError(f"required command is not available on PATH: {name}")
    return resolved


def run(
    command: str,
    *args: str,
    capture_output: bool = False,
    env: dict[str, str] | None = None,
    timeout: float | None = None,
) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        [executable(command), *args],
        cwd=ROOT,
        check=True,
        capture_output=capture_output,
        encoding="utf-8",
        errors="replace",
        env=os.environ | env if env is not None else None,
        timeout=timeout,
    )


def yarn(binary: str, *args: str) -> subprocess.CompletedProcess[str]:
    return run("yarn", "exec", binary, *args)
