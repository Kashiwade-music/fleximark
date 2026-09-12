from __future__ import annotations

import argparse
import shutil
from pathlib import Path

from _tools import ROOT, script_entrypoint, yarn


DIST = ROOT / "dist"
TEST_OUTPUT = ROOT / "out" / "test"
UNIT_TEST_OUTPUT = TEST_OUTPUT / "unit"
ELECTRON_TEST_OUTPUT = TEST_OUTPUT / "electron"
TYPE_OUTPUT = ROOT / "out" / "types"


def _mode_args(production: bool) -> list[str]:
    if production:
        return ["--minify"]
    return ["--sourcemap", "--sources-content=false"]


def extension_args(*, production: bool, watch: bool = False) -> list[str]:
    args = [
        "adapters/vscode/src/extension.mts",
        "--bundle",
        "--format=cjs",
        "--platform=node",
        "--outfile=dist/extension.cjs",
        "--external:vscode",
        "--loader:.css=text",
        f"--define:__DEV__={'false' if production else 'true'}",
        *_mode_args(production),
    ]
    if watch:
        args.append("--watch=forever")
    return args


def preview_args(*, production: bool, watch: bool = False) -> list[str]:
    args = [
        "web/preview-client/vscode-host.mts",
        "web/preview-client/browser-host.mts",
        "--bundle",
        "--format=iife",
        "--platform=browser",
        "--outdir=dist/web/preview-client",
        *_mode_args(production),
    ]
    if watch:
        args.append("--watch=forever")
    return args


def test_args(*, watch: bool = False) -> list[str]:
    args = [
        "test/extension.test.mts",
        "--bundle",
        "--platform=node",
        "--format=cjs",
        "--sourcemap",
        "--outfile=out/test/electron/extension.test.cjs",
        "--external:vscode",
        "--loader:.css=text",
    ]
    if watch:
        args.append("--watch=forever")
    return args


def pure_test_args() -> list[str]:
    return [
        "test/pure.test.mts",
        "--bundle",
        "--platform=node",
        "--format=cjs",
        "--sourcemap",
        "--outfile=out/test/unit/pure-tests.cjs",
        "--loader:.css=text",
    ]


def clean() -> None:
    shutil.rmtree(DIST, ignore_errors=True)
    shutil.rmtree(TEST_OUTPUT, ignore_errors=True)
    shutil.rmtree(TYPE_OUTPUT, ignore_errors=True)


def build_extension(*, production: bool) -> None:
    shutil.rmtree(DIST, ignore_errors=True)
    yarn("esbuild", *extension_args(production=production))
    yarn("esbuild", *preview_args(production=production))


def build_browser_client() -> None:
    output = ROOT / "web" / "preview-client" / "browser-host.js"
    yarn(
        "esbuild",
        "web/preview-client/browser-host.mts",
        "--bundle",
        "--format=iife",
        "--platform=browser",
        "--minify",
        "--outfile=web/preview-client/browser-host.js",
    )
    if not output.is_file() or output.stat().st_size == 0:
        raise RuntimeError("browser client bundle is empty")
    print(output.relative_to(ROOT).as_posix())


def build_tests() -> None:
    shutil.rmtree(TEST_OUTPUT, ignore_errors=True)
    build_electron_tests()
    build_pure_tests()


def build_electron_tests() -> None:
    shutil.rmtree(ELECTRON_TEST_OUTPUT, ignore_errors=True)
    yarn("esbuild", *test_args())


def build_pure_tests() -> None:
    shutil.rmtree(UNIT_TEST_OUTPUT, ignore_errors=True)
    yarn("esbuild", *pure_test_args())


def watch_commands() -> list[list[str]]:
    return [
        ["yarn", "exec", "esbuild", *extension_args(production=False, watch=True)],
        ["yarn", "exec", "esbuild", *preview_args(production=False, watch=True)],
    ]


def main() -> None:
    parser = argparse.ArgumentParser(description="Build FlexiMark JavaScript bundles")
    parser.add_argument(
        "target", choices=["browser", "clean", "extension", "tests"]
    )
    parser.add_argument("--production", action="store_true")
    args = parser.parse_args()

    if args.target == "browser":
        build_browser_client()
    elif args.target == "clean":
        clean()
    elif args.target == "extension":
        build_extension(production=args.production)
    else:
        build_tests()


if __name__ == "__main__":
    script_entrypoint(main)
