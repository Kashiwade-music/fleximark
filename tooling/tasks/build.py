from __future__ import annotations

import os
import re
import shutil
from pathlib import Path

from _tools import ROOT, yarn
from protocol_codegen import check_protocol_contract


DIST = ROOT / ".build" / "vscode"
TEST_OUTPUT = ROOT / ".build" / "tests"
UNIT_TEST_OUTPUT = TEST_OUTPUT / "unit"
ELECTRON_TEST_OUTPUT = TEST_OUTPUT / "electron"
TYPE_OUTPUT = ROOT / ".build" / "types"


def bundle_args(
    entry: str,
    output: str,
    *,
    platform: str,
    format: str,
    production: bool = False,
    watch: bool = False,
    external: tuple[str, ...] = (),
    aliases: tuple[str, ...] = (),
    css_text: bool = False,
) -> list[str]:
    args = [entry, "--bundle", f"--format={format}", f"--platform={platform}",
            f"--outfile={output}"]
    args.extend(["--minify"] if production else ["--sourcemap", "--sources-content=false"])
    args.extend(f"--external:{name}" for name in external)
    args.extend(f"--alias:{alias}" for alias in aliases)
    if css_text:
        args.append("--loader:.css=text")
    if watch:
        args.append("--watch=forever")
    return args


def extension_args(*, production: bool, watch: bool = False) -> list[str]:
    return bundle_args("adapters/vscode/src/extension.mts",
        ".build/vscode/product/dist/extension.cjs" if watch else ".build/vscode/dist/extension.cjs",
        platform="node", format="cjs", production=production, watch=watch,
        external=("vscode",), css_text=True)


def preview_args(*, production: bool, watch: bool = False) -> list[str]:
    return bundle_args("adapters/vscode/webview/vscode-host.mts",
        ".build/vscode/product/dist/webview/vscode-host.js" if watch else ".build/preview/vscode-host.js",
        platform="browser", format="iife", production=production, watch=watch)


def browser_args(*, production: bool, watch: bool = False) -> list[str]:
    return bundle_args("web/preview/browser-host.mts",
        ".build/vscode/product/runtime/resources/preview/browser-host.js" if watch else ".build/preview/browser-host.js",
        platform="browser", format="iife", production=production, watch=watch)


def workspace_args(*, production: bool, watch: bool = False) -> list[str]:
    return bundle_args("web/workspace-ui/index.mts",
        ".build/vscode/product/runtime/resources/workspace/workspace.js" if watch else ".build/workspace-ui/workspace.js",
        platform="browser", format="esm", production=production, watch=watch)


def test_args(*, watch: bool = False) -> list[str]:
    return bundle_args(".build/tests/generated/editor.test.mts",
        ".build/tests/electron/extension.test.cjs", platform="node", format="cjs",
        watch=watch, external=("vscode",), css_text=True)


def pure_test_args() -> list[str]:
    return bundle_args(".build/tests/generated/unit.test.mts", ".build/tests/unit/pure-tests.cjs",
        platform="node", format="cjs", css_text=True,
        aliases=("vscode=./tests/typescript/support/vscode-pure-stub.mts",))


def standalone_test_args() -> list[str]:
    return bundle_args(".build/tests/generated/standalone.test.mts",
        ".build/tests/electron/standalone.test.cjs", platform="node", format="cjs",
        external=("vscode",), css_text=True)


def discover_test_suites(root: Path | None = None) -> dict[str, list[Path]]:
    """Discover exported suites by the directory owning their execution host."""
    root = root or ROOT / "tests/typescript"
    groups: dict[str, list[Path]] = {"unit": [], "editor": [], "standalone": []}
    for path in sorted(root.rglob("*.test.mts")):
        source = path.read_text(encoding="utf-8")
        if not re.search(r"export\s+const\s+suiteName\b", source):
            continue
        if not re.search(r"export\s+function\s+suite\b", source):
            raise ValueError(f"test suite must export suite(): {path}")
        host = path.relative_to(root).parts[0]
        if host not in groups:
            raise ValueError(f"test suite must reside in a host directory: {path}")
        groups[host].append(path)
    return groups


def test_entry_source(host: str, suites: list[Path], destination: Path) -> str:
    lines = []
    if host == "unit":
        lines.append('import { afterEach, beforeEach, describe, it } from "node:test";')
    elif host == "editor":
        lines.extend(['import * as assert from "node:assert/strict";',
                      'import * as vscode from "vscode";'])
    for index, path in enumerate(suites):
        module = os.path.relpath(path.with_suffix(".mjs"), destination.parent).replace("\\", "/")
        if not module.startswith("."):
            module = "./" + module
        lines.append(f'import * as suite{index} from "{module}";')
    if host == "unit":
        lines.append('Object.assign(globalThis, { setup: beforeEach, teardown: afterEach, test: it });')
        lines.extend(f"describe(suite{index}.suiteName, suite{index}.suite);" for index in range(len(suites)))
    else:
        title = "Extension Test Suite" if host == "editor" else "Standalone File Test Suite"
        lines.append(f'suite("{title}", () => {{')
        if host == "editor":
            lines.extend(['  suiteSetup(async () => {',
                          '    const extension = vscode.extensions.getExtension("Kashiwade.fleximark");',
                          '    assert.ok(extension);', '    await extension.activate();', '  });'])
        lines.extend(f"  suite(suite{index}.suiteName, suite{index}.suite);" for index in range(len(suites)))
        lines.append("});")
    return "\n".join(lines) + "\n"


def generate_test_entries() -> dict[str, Path]:
    output = TEST_OUTPUT / "generated"
    output.mkdir(parents=True, exist_ok=True)
    entries = {}
    for host, suites in discover_test_suites().items():
        entry = output / f"{host}.test.mts"
        entry.write_text(test_entry_source(host, suites, entry), encoding="utf-8")
        entries[host] = entry
    return entries


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
    yarn("esbuild", *browser_args(production=True))
    if not output.is_file() or output.stat().st_size == 0:
        raise RuntimeError("browser client bundle is empty")
    print(output.relative_to(ROOT).as_posix())
    yarn("esbuild", "web/preview/fleximark.css", "--bundle", "--minify", "--outfile=.build/preview/fleximark.css")
    shutil.copy2(ROOT / "web" / "preview" / "katex.css", output.parent / "katex.css")


def build_tests() -> None:
    shutil.rmtree(TEST_OUTPUT, ignore_errors=True)
    build_electron_tests()
    build_pure_tests()


def build_workspace_ui() -> None:
    yarn("esbuild", *workspace_args(production=True))
    shutil.copy2(ROOT / "web" / "workspace-ui" / "workspace.css", ROOT / ".build" / "workspace-ui" / "workspace.css")


def build_electron_tests() -> None:
    shutil.rmtree(ELECTRON_TEST_OUTPUT, ignore_errors=True)
    generate_test_entries()
    yarn("esbuild", *test_args())
    yarn("esbuild", *standalone_test_args())


def build_pure_tests() -> None:
    shutil.rmtree(UNIT_TEST_OUTPUT, ignore_errors=True)
    generate_test_entries()
    yarn("esbuild", *pure_test_args())


def watch_commands() -> list[list[str]]:
    return [
        ["yarn", "exec", "esbuild", *factory(production=False, watch=True)]
        for factory in (extension_args, preview_args, browser_args, workspace_args)
    ]
