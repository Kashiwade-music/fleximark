from __future__ import annotations

import argparse
import hashlib
import json

from _tools import ROOT


TARGETS = [
    ("linux", "x64", "fleximarkd"),
    ("linux", "arm64", "fleximarkd"),
    ("darwin", "x64", "fleximarkd"),
    ("darwin", "arm64", "fleximarkd"),
    ("win32", "x64", "fleximarkd.exe"),
    ("win32", "arm64", "fleximarkd.exe"),
]


def create_manifest(*, require_all: bool = False) -> None:
    artifacts: list[dict[str, str]] = []
    for platform, arch, executable in TARGETS:
        relative_path = f"bin/{platform}-{arch}/{executable}"
        artifact = ROOT / relative_path
        if not artifact.is_file():
            continue
        artifacts.append(
            {
                "platform": platform,
                "arch": arch,
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
            {"schemaVersion": 1, "protocolVersion": 1, "artifacts": artifacts},
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
    main()
