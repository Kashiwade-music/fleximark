from __future__ import annotations
import build as javascript_build
from _tools import ROOT, run, yarn
from distribution import assemble_runtime, generate_versions, host_target
from protocol_codegen import check_protocol_contract
import task_checks
import task_js
import task_plan
import task_products

def runtime_build_steps() -> list[task_plan.Step]:
    return [
        task_plan.Step('Synchronize product manifests from release.toml', generate_versions),
        task_plan.Step(
            'Build native daemon and CLI (release, current platform)',
            task_products.build_daemon,
        ),
        task_plan.Step(
            'Build browser preview JavaScript and copy its CSS',
            javascript_build.build_browser_client,
        ),
        task_plan.Step(
            'Build browser workspace JavaScript and copy its CSS',
            javascript_build.build_workspace_ui,
        ),
        task_plan.Step(
            'Assemble runtime directory, ZIP and SHA-256 checksum',
            lambda: assemble_runtime(host_target()),
        ),
    ]

def build_steps(target: str, *, check_contract: bool=True) -> list[task_plan.Step]:
    steps = [task_plan.Step('Check generated protocol against the Rust contract', check_protocol_contract)] if check_contract else []
    if target == 'core':
        return steps + [
            task_plan.Step(
                'Build Rust document and application libraries (debug)',
                lambda: task_products.build_core(()),
            ),
        ]
    steps += runtime_build_steps()
    if target in {'vscode', 'all'}:
        steps.append(
            task_plan.Step(
                'Build and stage the VS Code extension with that runtime',
                lambda: task_products.build_vscode_product(task_products.runtime_archive()),
            ),
        )
    if target in {'zed', 'all'}:
        steps.append(
            task_plan.Step(
                'Build Zed WASM extension; stage it and create ZIP/checksum',
                task_products.build_zed_product,
            ),
        )
    return steps

def check_steps() -> list[task_plan.Step]:
    return [
        task_plan.Step(
            'Check product version freshness against release.toml',
            lambda: task_checks.check_generated_freshness(
                generate_versions,
                [ROOT / name for name in ('adapters/vscode/package.json', 'adapters/zed/extension.toml', 'apps/server/Cargo.toml', 'apps/cli/Cargo.toml', 'Cargo.lock')],
            ),
        ),
        task_plan.Step('Check generated protocol against the Rust contract', check_protocol_contract),
        task_plan.Step(
            'Check root/Zed Rust architecture and diagram freshness',
            lambda: task_checks.check_generated_freshness(
                task_checks.check_architecture,
                [ROOT / name for name in ('architecture', 'adapters/zed/architecture')],
            ),
        ),
        task_plan.Step(
            'Check Python task syntax',
            lambda: run('python', '-m', 'compileall', '-q', 'tooling/tasks'),
        ),
        task_plan.Step('Check all TypeScript project types', lambda: yarn('tsc', '-b')),
        task_plan.Step(
            'Lint TypeScript/JavaScript sources',
            lambda: yarn(
                'eslint',
                'adapters/vscode',
                'web',
                'contracts/typescript',
                'tests/typescript',
                'tooling/tasks',
            ),
        ),
        task_plan.Step('Check generated English localization freshness', task_checks.check_localization),
        task_plan.Step(
            'Check root Rust formatting',
            lambda: run('cargo', 'fmt', '--all', '--', '--check'),
        ),
        task_plan.Step(
            'Check Zed Rust formatting',
            lambda: run('cargo', 'fmt', '--manifest-path', 'adapters/zed/Cargo.toml', '--', '--check'),
        ),
        task_plan.Step(
            'Lint Zed Rust code and test targets',
            lambda: run(
                'cargo',
                'clippy',
                '--manifest-path',
                'adapters/zed/Cargo.toml',
                '--target-dir',
                'target/zed',
                '--all-targets',
                '--locked',
                '--',
                '-D',
                'warnings',
            ),
        ),
        task_plan.Step(
            'Lint root Rust workspace code and test targets',
            lambda: run('cargo', 'clippy', '--workspace', '--all-targets', '--locked', '--', '-D', 'warnings'),
        ),
    ]

def unit_steps(*, check_contract: bool=True) -> list[task_plan.Step]:
    contract = [task_plan.Step('Check generated protocol', check_protocol_contract)] if check_contract else []
    return contract + [
        task_plan.Step('Run Python and Node build/release tooling tests', task_js.python_test),
        task_plan.Step(
            'Run root Rust workspace tests (including native integration)',
            lambda: run('cargo', 'test', '--workspace', '--all-targets', '--locked'),
        ),
        task_plan.Step(
            'Run native Zed adapter tests',
            lambda: run(
                'cargo',
                'test',
                '--manifest-path',
                'adapters/zed/Cargo.toml',
                '--target-dir',
                'target/zed',
                '--locked',
            ),
        ),
        task_plan.Step(
            'Check protocol; compile and run Node protocol/preview/adapter tests',
            lambda: task_js.pure_test(()),
        ),
    ]
