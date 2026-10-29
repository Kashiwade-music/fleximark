from __future__ import annotations
import os
import shutil
import subprocess
from pathlib import Path
from collections.abc import Sequence
import build as javascript_build
from _tools import ROOT, executable, run
from distribution import TARGETS, assemble_vscode, build_native, host_target, metadata, staging, reset_output, archive_directory
from check_performance_budgets import check_performance_budgets
from smoke_vsix import smoke_vsix
import release_artifact
import task_js

def build_daemon() -> None:
    build_native(host_target())

def build_vscode_product(archive: Path) -> None:
    javascript_build.build_extension(production=True)
    assemble_vscode(host_target(), archive)

def build_zed_product() -> None:
    run(
        'cargo',
        'build',
        '--manifest-path',
        'adapters/zed/Cargo.toml',
        '--release',
        '--target',
        'wasm32-wasip2',
        '--target-dir',
        str(ROOT / 'target' / 'zed'),
        '--locked',
    )
    output = staging(host_target(), 'zed')
    reset_output(output)
    shutil.copy2(ROOT / 'adapters' / 'zed' / 'extension.toml', output / 'extension.toml')
    shutil.copy2(
        ROOT / 'target' / 'zed' / 'wasm32-wasip2' / 'release' / 'fleximark_zed.wasm',
        output / 'extension.wasm',
    )
    languages = ROOT / 'adapters' / 'zed' / 'languages'
    if languages.is_dir():
        shutil.copytree(languages, output / 'languages')
    archive = output.parent / f"fleximark-zed-{metadata()['version']}.zip"
    archive_directory(output, archive)

def build_core(_: Sequence[str]) -> None:
    run(
        'cargo',
        'build',
        '--locked',
        '-p',
        'fleximark-model',
        '-p',
        'fleximark-parser',
        '-p',
        'fleximark-render-html',
        '-p',
        'fleximark-engine',
        '-p',
        'fleximark-language',
        '-p',
        'fleximark-application',
    )

def verify_built_product(package_args: Sequence[str]=()) -> None:
    """Host gates that require the assembled native and editor product."""
    check_performance_budgets()
    task_js.compile_tests()
    task_js.run_editor_integration()
    vsix = package_identified_product(package_args)
    smoke_vsix(vsix)

def package_identified_product(package_args: Sequence[str]=()) -> Path:
    """Package the assembled VSIX and verify its exact source identity."""
    vsix = package_built_product(package_args)
    source_head = os.environ.get('SOURCE_GIT_HEAD') or subprocess.check_output([executable('git'), 'rev-parse', 'HEAD'], cwd=ROOT, text=True).strip()
    tag = f"v{metadata()['version']}"
    identity = vsix.with_suffix('.vsix.identity.json')
    release_artifact.create_identity(
        vsix,
        identity,
        expected_version=metadata()['version'],
        git_tag=tag,
        source_git_head=source_head,
    )
    release_artifact.verify_identity(
        vsix,
        identity,
        expected_git_tag=tag,
        expected_source_git_head=source_head,
    )
    return vsix

def package_built_product(args: Sequence[str]) -> Path:
    target = host_target()
    output = ROOT / '.artifacts' / metadata()['version'] / target
    run(
        'node',
        str(ROOT / 'node_modules' / '@vscode' / 'vsce' / 'vsce'),
        'package',
        '--no-dependencies',
        '--target',
        next((name for name, rust in TARGETS.items() if rust == target)),
        '--out',
        str(output / f"fleximark-{metadata()['version']}-{target}.vsix"),
        *args,
        cwd=output / 'vscode',
    )
    return output / f"fleximark-{metadata()['version']}-{target}.vsix"

def runtime_archive() -> Path:
    return ROOT / '.artifacts' / metadata()['version'] / host_target() / f"fleximark-{metadata()['version']}-{host_target()}.zip"
