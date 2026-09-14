from __future__ import annotations

import platform
import shutil
import sys

from _targets import current_target, normalize_arch, normalize_platform
from _tools import ROOT, script_entrypoint


def platform_name() -> str:
    return normalize_platform(sys.platform)


def architecture_name() -> str:
    return normalize_arch(platform.machine())


def stage_daemon() -> None:
    target = current_target(sys.platform, platform.machine())
    source = ROOT / "target" / "release" / target.executable
    destination = ROOT.joinpath(*target.bin_relative_path.parts)
    destination.parent.mkdir(parents=True, exist_ok=True)
    shutil.copy2(source, destination)
    target.normalize_executable_mode(destination)


if __name__ == "__main__":
    script_entrypoint(stage_daemon)
