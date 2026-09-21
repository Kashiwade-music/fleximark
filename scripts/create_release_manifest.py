from __future__ import annotations

import argparse
import hashlib
import json

from _targets import TARGETS
from _tools import ROOT, script_entrypoint


def create_manifest(*, require_all: bool = False) -> None:
    artifacts: list[dict[str, str]] = []
    for target in TARGETS:
        relative_path = target.bin_relative_path.as_posix()
        artifact = ROOT / relative_path
        if not artifact.is_file():
            continue
        target.normalize_executable_mode(artifact)
        artifacts.append(
            {
                "platform": target.platform,
                "arch": target.arch,
                "path": relative_path,
                "sha256": hashlib.sha256(artifact.read_bytes()).hexdigest(),
            }
        )
    if require_all and len(artifacts) != len(TARGETS):
        raise RuntimeError("release requires Windows, macOS, and Linux daemons")

    output = ROOT / "bin" / "manifest.json"
    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_text(
        json.dumps(
            {"schemaVersion": 1, "protocolVersion": 2, "artifacts": artifacts},
            indent=2,
        )
        + "\n",
        encoding="utf-8",
    )


def main() -> None:
    parser = argparse.ArgumentParser(description="Create the release daemon manifest")
    parser.add_argument("--require-all", action="store_true")
    args = parser.parse_args()
    create_manifest(require_all=args.require_all)


if __name__ == "__main__":
    script_entrypoint(main)
