from __future__ import annotations

from collections.abc import Sequence

from _tools import run


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
    run("cargo", "xtask", "fixtures", mode)


def generate_protocol_contract(args: Sequence[str] = ()) -> None:
    if args:
        raise RuntimeError("protocol-generate does not accept arguments")
    run_codegen("generate")


def check_protocol_contract(args: Sequence[str] = ()) -> None:
    if args:
        raise RuntimeError("protocol-check does not accept arguments")
    run_codegen("check")
