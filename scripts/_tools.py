from __future__ import annotations

import os
import shutil
import subprocess
import sys
from collections.abc import Callable, Sequence
from pathlib import Path


ROOT = Path(__file__).resolve().parent.parent
WINDOWS_PATHEXT_DEFAULT = ".COM;.EXE;.BAT;.CMD"


def windows_executable_candidates(
    name: str,
    *,
    path: str,
    pathext: str | None,
    path_separator: str,
) -> tuple[Path, ...]:
    if Path(name).suffix:
        return ()
    extensions = (
        WINDOWS_PATHEXT_DEFAULT if pathext is None else pathext
    ).split(";")
    return tuple(
        Path(directory) / f"{name}{extension.lower()}"
        for directory in path.split(path_separator)
        for extension in extensions
    )


def resolve_executable(
    name: str,
    *,
    windows: bool,
    path: str,
    pathext: str | None,
    path_separator: str,
    is_file: Callable[[Path], bool],
    which: Callable[[str], str | None],
) -> str:
    if windows:
        for candidate in windows_executable_candidates(
            name,
            path=path,
            pathext=pathext,
            path_separator=path_separator,
        ):
            if is_file(candidate):
                return str(candidate)
    resolved = which(name)
    if resolved is None:
        raise RuntimeError(f"required command is not available on PATH: {name}")
    return resolved


def executable(name: str) -> str:
    return resolve_executable(
        name,
        windows=os.name == "nt",
        path=os.environ.get("PATH", ""),
        pathext=os.environ.get("PATHEXT"),
        path_separator=os.pathsep,
        is_file=Path.is_file,
        which=shutil.which,
    )


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


def _decode_process_output(value: str | bytes | None) -> str:
    if value is None:
        return ""
    if isinstance(value, bytes):
        return value.decode("utf-8", errors="replace")
    return value


def _format_command(command: str | bytes | Sequence[str | bytes]) -> str:
    if isinstance(command, bytes):
        return command.decode("utf-8", errors="replace")
    if isinstance(command, str):
        return command
    return subprocess.list2cmdline(
        [
            item.decode("utf-8", errors="replace")
            if isinstance(item, bytes)
            else str(item)
            for item in command
        ]
    )


def format_process_error(
    error: subprocess.CalledProcessError | subprocess.TimeoutExpired,
) -> str:
    command = _format_command(error.cmd)
    if isinstance(error, subprocess.TimeoutExpired):
        summary = f"command timed out after {error.timeout}s: {command}"
    else:
        summary = f"command failed with exit code {error.returncode}: {command}"

    details = []
    stdout = _decode_process_output(error.stdout)
    stderr = _decode_process_output(error.stderr)
    if stdout:
        details.append(f"stdout:\n{stdout.rstrip(chr(13) + chr(10))}")
    if stderr:
        details.append(f"stderr:\n{stderr.rstrip(chr(13) + chr(10))}")
    return "\n".join([summary, *details])


def script_entrypoint(main: Callable[[], None]) -> None:
    try:
        main()
    except (subprocess.CalledProcessError, subprocess.TimeoutExpired) as error:
        print(f"error: {format_process_error(error)}", file=sys.stderr)
        raise SystemExit(1) from error
    except RuntimeError as error:
        print(f"error: {error}", file=sys.stderr)
        raise SystemExit(1) from error
    return None
