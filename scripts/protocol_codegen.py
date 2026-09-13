from __future__ import annotations

import argparse
from collections.abc import Sequence

from _tools import run, script_entrypoint


CODEGEN_PACKAGE = "fleximark-protocol-codegen"


def run_codegen(mode: str) -> None:
    if mode not in {"generate", "check"}:
        raise ValueError(f"unsupported protocol codegen mode: {mode}")
    run(
        "cargo",
        "run",
        "--locked",
        "-p",
        CODEGEN_PACKAGE,
        "--",
        mode,
    )


def generate_protocol_contract(args: Sequence[str] = ()) -> None:
    if args:
        raise RuntimeError("protocol-generate does not accept arguments")
    run_codegen("generate")


def check_protocol_contract(args: Sequence[str] = ()) -> None:
    if args:
        raise RuntimeError("protocol-check does not accept arguments")
    run_codegen("check")


def main() -> None:
    parser = argparse.ArgumentParser(
        description="Generate or check Rust-owned protocol artifacts"
    )
    parser.add_argument("mode", choices=("generate", "check"))
    args = parser.parse_args()
    run_codegen(args.mode)


if __name__ == "__main__":
    script_entrypoint(main)
