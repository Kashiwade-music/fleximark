"""Verify the complete release set before any asset is published."""
from __future__ import annotations

import argparse
import zipfile
import tomllib
from pathlib import Path

from distribution import TARGETS, digest, metadata
from release_artifact import runtime_manifest_artifacts, _json_object, verify_identity
from _tools import script_entrypoint


def verify(root: Path, source_git_head: str, tag: str) -> None:
    version = metadata()["version"]
    if tag != f"v{version}":
        raise RuntimeError("release tag differs from release.toml")
    archives = list(root.rglob(f"fleximark-{version}-*.zip"))
    vsixes = list(root.rglob("*.vsix"))
    if len(archives) != len(TARGETS) or len(vsixes) != len(TARGETS):
        raise RuntimeError("release must contain one runtime and VSIX for every distribution target")
    zed = list(root.rglob(f"fleximark-zed-{version}.zip"))
    if len(zed) != 1:
        raise RuntimeError("release must contain exactly one Zed editor product")
    if zed[0].with_suffix(".zip.sha256").read_text(encoding="utf-8") != f"{digest(zed[0])}  {zed[0].name}\n":
        raise RuntimeError("Zed product archive hash mismatch")
    with zipfile.ZipFile(zed[0]) as extension:
        if not {"extension.toml", "extension.wasm"}.issubset(extension.namelist()):
            raise RuntimeError("Zed product is incomplete")
        if tomllib.loads(extension.read("extension.toml").decode())["version"] != version or not extension.read("extension.wasm").startswith(b"\0asm"):
            raise RuntimeError("Zed product has invalid version or WASM binary")
    for target in TARGETS.values():
        name = f"fleximark-{version}-{target}"
        runtime = next((p for p in archives if p.name == name + ".zip"), None)
        vsix = next((p for p in vsixes if p.name == name + ".vsix"), None)
        if runtime is None or vsix is None:
            raise RuntimeError(f"missing release target: {target}")
        checksum = runtime.with_suffix(".zip.sha256").read_text(encoding="utf-8")
        if checksum != f"{digest(runtime)}  {runtime.name}\n":
            raise RuntimeError("runtime archive hash mismatch")
        verify_identity(vsix, vsix.with_suffix(".vsix.identity.json"), expected_git_tag=tag, expected_source_git_head=source_git_head)
        with zipfile.ZipFile(runtime) as archive, zipfile.ZipFile(vsix) as extension:
            payload = archive.read("manifest.json")
            manifest = _json_object(payload, "release runtime manifest")
            if manifest.get("version") != version or manifest.get("target") != target:
                raise RuntimeError("runtime identity does not match release")
            artifacts = runtime_manifest_artifacts(payload, "release runtime manifest")
            expected = {item["path"].removeprefix("runtime/") for item in artifacts} | {"manifest.json"}
            if set(archive.namelist()) != expected or len(archive.namelist()) != len(expected):
                raise RuntimeError("runtime archive contains unexpected or duplicated files")
            for path in expected:
                if archive.read(path) != extension.read("extension/runtime/" + path):
                    raise RuntimeError("VSIX runtime differs from shared native archive")


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("command", choices=("verify",))
    parser.add_argument("--root", type=Path, required=True)
    parser.add_argument("--source-git-head", required=True)
    parser.add_argument("--tag", required=True)
    args = parser.parse_args()
    verify(args.root, args.source_git_head, args.tag)


if __name__ == "__main__":
    script_entrypoint(main)
