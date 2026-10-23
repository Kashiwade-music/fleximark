from __future__ import annotations

import argparse
import shutil
from pathlib import Path

from _tools import ROOT, script_entrypoint, yarn
from protocol_codegen import check_protocol_contract


DIST = ROOT / ".build" / "vscode"
TEST_OUTPUT = ROOT / ".build" / "tests"
UNIT_TEST_OUTPUT = TEST_OUTPUT / "unit"
ELECTRON_TEST_OUTPUT = TEST_OUTPUT / "electron"
TYPE_OUTPUT = ROOT / ".build" / "types"


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
        "--outfile=.build/vscode/dist/extension.cjs",
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
        "adapters/vscode/webview/vscode-host.mts",
        "--bundle",
        "--format=iife",
        "--platform=browser",
        "--outdir=.build/preview",
        "--entry-names=[name]",
        *_mode_args(production),
    ]
    if watch:
        args.append("--watch=forever")
    return args


def test_args(*, watch: bool = False) -> list[str]:
    args = [
        "tests/typescript/extension.test.mts",
        "--bundle",
        "--platform=node",
        "--format=cjs",
        "--sourcemap",
        "--outfile=.build/tests/electron/extension.test.cjs",
        "--external:vscode",
        "--loader:.css=text",
    ]
    if watch:
        args.append("--watch=forever")
    return args


def pure_test_args() -> list[str]:
    return [
        "tests/typescript/pure.test.mts",
        "--bundle",
        "--platform=node",
        "--format=cjs",
        "--sourcemap",
        "--outfile=.build/tests/unit/pure-tests.cjs",
        "--alias:vscode=./tests/typescript/adapter/vscode-pure-stub.mts",
        "--loader:.css=text",
    ]


def standalone_test_args() -> list[str]:
    return [
        argument.replace("tests/typescript/extension.test.mts", "tests/typescript/standalone.test.mts")
        .replace("electron/extension.test.cjs", "electron/standalone.test.cjs")
        for argument in test_args()
    ]


def clean() -> None:
    shutil.rmtree(DIST, ignore_errors=True)
    shutil.rmtree(TEST_OUTPUT, ignore_errors=True)
    shutil.rmtree(TYPE_OUTPUT, ignore_errors=True)
    shutil.rmtree(ROOT / ".build" / "preview", ignore_errors=True)
    shutil.rmtree(ROOT / ".build" / "workspace-ui", ignore_errors=True)


def build_extension(*, production: bool) -> None:
    shutil.rmtree(DIST, ignore_errors=True)
    yarn("esbuild", *extension_args(production=production))
    yarn("esbuild", *preview_args(production=production))


def build_browser_client() -> None:
    output = ROOT / ".build" / "preview" / "browser-host.js"
    yarn(
        "esbuild",
        "web/preview/browser-host.mts",
        "--bundle",
        "--format=iife",
        "--platform=browser",
        "--minify",
        "--outfile=.build/preview/browser-host.js",
    )
    if not output.is_file() or output.stat().st_size == 0:
        raise RuntimeError("browser client bundle is empty")
    print(output.relative_to(ROOT).as_posix())
    for name in ("fleximark.css", "katex.css"):
        shutil.copy2(ROOT / "web" / "preview" / name, output.parent / name)


def build_tests() -> None:
    shutil.rmtree(TEST_OUTPUT, ignore_errors=True)
    build_electron_tests()
    build_pure_tests()


def build_workspace_ui() -> None:
    yarn("esbuild", "web/workspace-ui/index.mts", "--bundle", "--format=esm", "--platform=browser", "--minify", "--outfile=.build/workspace-ui/workspace.js")
    shutil.copy2(ROOT / "web" / "workspace-ui" / "workspace.css", ROOT / ".build" / "workspace-ui" / "workspace.css")


def build_electron_tests() -> None:
    shutil.rmtree(ELECTRON_TEST_OUTPUT, ignore_errors=True)
    yarn("esbuild", *test_args())
    yarn("esbuild", *standalone_test_args())


def build_pure_tests() -> None:
    shutil.rmtree(UNIT_TEST_OUTPUT, ignore_errors=True)
    yarn("esbuild", *pure_test_args())


def watch_commands() -> list[list[str]]:
    extension = extension_args(production=False, watch=True)
    extension[extension.index("--outfile=.build/vscode/dist/extension.cjs")] = "--outfile=.build/vscode/product/dist/extension.cjs"
    return [
        ["yarn", "exec", "esbuild", *extension],
        ["yarn", "exec", "esbuild", "adapters/vscode/webview/vscode-host.mts", "--bundle", "--format=iife", "--platform=browser", "--sourcemap", "--outfile=.build/vscode/product/dist/webview/vscode-host.js", "--watch=forever"],
        ["yarn", "exec", "esbuild", "web/preview/browser-host.mts", "--bundle", "--format=iife", "--platform=browser", "--sourcemap", "--outfile=.build/vscode/product/runtime/resources/preview/browser-host.js", "--watch=forever"],
        ["yarn", "exec", "esbuild", "web/workspace-ui/index.mts", "--bundle", "--format=esm", "--platform=browser", "--sourcemap", "--outfile=.build/vscode/product/runtime/resources/workspace/workspace.js", "--watch=forever"],
    ]


def main() -> None:
    parser = argparse.ArgumentParser(description="Build FlexiMark JavaScript bundles")
    parser.add_argument(
        "target", choices=["browser", "workspace", "clean", "extension", "tests"]
    )
    parser.add_argument("--production", action="store_true")
    args = parser.parse_args()

    if args.target == "browser":
        check_protocol_contract()
        build_browser_client()
    elif args.target == "workspace":
        build_workspace_ui()
    elif args.target == "clean":
        clean()
    elif args.target == "extension":
        check_protocol_contract()
        build_extension(production=args.production)
    else:
        check_protocol_contract()
        build_tests()


if __name__ == "__main__":
    script_entrypoint(main)
