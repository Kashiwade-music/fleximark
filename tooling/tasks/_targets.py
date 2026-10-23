from __future__ import annotations

import platform
import sys
from dataclasses import dataclass
from pathlib import Path, PurePosixPath


@dataclass(frozen=True, slots=True)
class Target:
    platform: str
    arch: str
    executable: str

    @property
    def bin_relative_path(self) -> PurePosixPath:
        return PurePosixPath("bin") / f"{self.platform}-{self.arch}" / self.executable

    def normalize_executable_mode(self, artifact: Path) -> None:
        if self.platform != "win32":
            artifact.chmod(0o755)


TARGETS: tuple[Target, ...] = (
    Target("linux", "x64", "fleximarkd"),
    Target("linux", "arm64", "fleximarkd"),
    Target("darwin", "x64", "fleximarkd"),
    Target("darwin", "arm64", "fleximarkd"),
    Target("win32", "x64", "fleximarkd.exe"),
    Target("win32", "arm64", "fleximarkd.exe"),
)


def normalize_platform(reported: str) -> str:
    if reported == "win32":
        return "win32"
    if reported == "darwin":
        return "darwin"
    if reported.startswith("linux"):
        return "linux"
    raise RuntimeError(f"unsupported platform: {reported}")


def normalize_arch(reported: str) -> str:
    machine = reported.lower()
    if machine in {"amd64", "x86_64"}:
        return "x64"
    if machine in {"arm64", "aarch64"}:
        return "arm64"
    raise RuntimeError(f"unsupported architecture: {machine}")


def find_target(platform_name: str, architecture_name: str) -> Target:
    for target in TARGETS:
        if target.platform == platform_name and target.arch == architecture_name:
            return target
    raise RuntimeError(
        f"unsupported target: {platform_name}-{architecture_name}"
    )


def current_target(
    reported_platform: str | None = None,
    reported_architecture: str | None = None,
) -> Target:
    return find_target(
        normalize_platform(sys.platform if reported_platform is None else reported_platform),
        normalize_arch(
            platform.machine()
            if reported_architecture is None
            else reported_architecture
        ),
    )
