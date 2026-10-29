"""Own Python distribution metadata and strict runtime manifest validation."""
from __future__ import annotations

import hashlib
import json
import os
import re
import stat
import tomllib
from pathlib import Path
from typing import Any

from _tools import ROOT

SPEC = json.loads((ROOT / "contracts/distribution/distribution.json").read_text(encoding="utf-8"))
TARGETS = {target["vscode"]: target["triple"] for target in SPEC["targets"]}
SEMVER_PATTERN = re.compile(
    r"(?:0|[1-9][0-9]*)\."
    r"(?:0|[1-9][0-9]*)\."
    r"(?:0|[1-9][0-9]*)"
    r"(?:-(?:0|[1-9][0-9]*|[0-9A-Za-z-]*[A-Za-z-][0-9A-Za-z-]*)"
    r"(?:\.(?:0|[1-9][0-9]*|[0-9A-Za-z-]*[A-Za-z-][0-9A-Za-z-]*))*)?"
    r"(?:\+[0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*)?\Z"
)
SHA256_PATTERN = re.compile(r"[0-9a-f]{64}\Z")


def metadata() -> dict:
    return tomllib.loads((ROOT / "release.toml").read_text(encoding="utf-8"))


def target_spec(triple: str) -> dict:
    for target in SPEC["targets"]:
        if target["triple"] == triple:
            return target
    raise RuntimeError(f"unsupported distribution target: {triple}")


def runtime_paths(triple: str) -> list[str]:
    suffix = target_spec(triple)["executableSuffix"]
    return [file["path"].replace("{exe}", suffix) for file in SPEC["files"]]


def normalize_platform(reported: str) -> str:
    normalized = "linux" if reported.startswith("linux") else reported
    if any(target["os"] == normalized for target in SPEC["targets"]):
        return normalized
    raise RuntimeError(f"unsupported platform: {reported}")


def normalize_arch(reported: str) -> str:
    normalized = {"amd64": "x64", "x86_64": "x64", "aarch64": "arm64"}.get(reported.lower(), reported.lower())
    if any(target["arch"] == normalized for target in SPEC["targets"]):
        return normalized
    raise RuntimeError(f"unsupported architecture: {reported}")


def read_manifest(payload: bytes, label: str, *, target: str | None = None, version: str | None = None) -> dict[str, Any]:
    if len(payload) > SPEC["limits"]["manifestBytes"]:
        raise RuntimeError(f"{label} exceeds size limit")

    def unique_object(items: list[tuple[str, Any]]) -> dict:
        value: dict = {}
        for key, item in items:
            if key in value:
                raise RuntimeError(f"{label} contains a repeated field")
            value[key] = item
        return value

    try:
        manifest = json.loads(payload.decode("utf-8"), object_pairs_hook=unique_object)
    except (UnicodeError, ValueError) as error:
        raise RuntimeError(f"{label} is invalid JSON") from error
    if not isinstance(manifest, dict) or set(manifest) != {"schemaVersion", "protocolVersion", "version", "target", "files"}:
        raise RuntimeError(f"{label} has unexpected fields")
    if type(manifest["schemaVersion"]) is not int or manifest["schemaVersion"] != 1 or type(manifest["protocolVersion"]) is not int or manifest["protocolVersion"] != metadata()["protocol_version"]:
        raise RuntimeError(f"{label} has unsupported versions or target")
    if not isinstance(manifest["target"], str) or manifest["target"] not in TARGETS.values() or target is not None and manifest["target"] != target:
        raise RuntimeError(f"{label} has unsupported versions or target")
    if not isinstance(manifest["version"], str) or not SEMVER_PATTERN.fullmatch(manifest["version"]) or version is not None and manifest["version"] != version:
        raise RuntimeError(f"{label} has invalid version")
    files = manifest["files"]
    if not isinstance(files, dict) or set(files) != set(runtime_paths(manifest["target"])):
        raise RuntimeError(f"{label} must contain canonical runtime files")
    if any(not isinstance(checksum, str) or not SHA256_PATTERN.fullmatch(checksum) for checksum in files.values()):
        raise RuntimeError(f"{label} has invalid SHA-256")
    return manifest


def linked(metadata: os.stat_result) -> bool:
    return stat.S_ISLNK(metadata.st_mode) or bool(getattr(metadata, "st_file_attributes", 0) & 0x400)


def read_payload(root: Path, relative: str, maximum: int) -> bytes:
    if "\\" in relative or any(not part or part in {".", ".."} or ":" in part for part in relative.split("/")):
        raise RuntimeError("invalid runtime payload path")
    current = root
    for part in relative.split("/"):
        current /= part
        if linked(current.lstat()):
            raise RuntimeError("runtime payload contains a link")
    if not current.resolve().is_relative_to(root.resolve()):
        raise RuntimeError("runtime payload escapes distribution")
    with current.open("rb") as stream:
        metadata = os.fstat(stream.fileno())
        if not stat.S_ISREG(metadata.st_mode) or not 0 < metadata.st_size <= maximum:
            raise RuntimeError("runtime payload is missing, empty or oversized")
        data = stream.read(maximum + 1)
    if len(data) > maximum:
        raise RuntimeError("runtime payload grew beyond its size limit")
    return data


def verify_runtime(root: Path, target: str) -> None:
    if linked(root.lstat()) or not root.is_dir():
        raise RuntimeError("runtime root must be a regular directory")
    root = root.resolve()
    manifest = read_manifest(read_payload(root, "manifest.json", SPEC["limits"]["manifestBytes"]), "runtime manifest", target=target, version=metadata()["version"])
    total = 0
    for relative, checksum in manifest["files"].items():
        data = read_payload(root, relative, SPEC["limits"]["fileBytes"])
        total += len(data)
        if total > SPEC["limits"]["totalBytes"]:
            raise RuntimeError("runtime exceeds total size limit")
        if hashlib.sha256(data).hexdigest() != checksum:
            raise RuntimeError(f"runtime checksum mismatch: {relative}")
    allowed = set(manifest["files"]) | {"manifest.json"}
    for directory, directories, files in os.walk(root, followlinks=False):
        for name in directories + files:
            path = Path(directory) / name
            relative = path.relative_to(root).as_posix()
            if linked(path.lstat()):
                raise RuntimeError("runtime tree contains a link")
            if path.is_dir():
                if not any(file.startswith(relative + "/") for file in allowed):
                    raise RuntimeError("runtime tree contains an unlisted directory")
            elif relative not in allowed:
                raise RuntimeError("runtime tree contains an unlisted file")
