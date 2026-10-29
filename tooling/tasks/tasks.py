from __future__ import annotations
import argparse
import os
from pathlib import Path
from collections.abc import Sequence
from _tools import run, script_entrypoint
from distribution import metadata, host_target, generate_versions
from l10n_export import export_localization
from protocol_codegen import generate_protocol_contract
import task_plan
import task_watch
import task_js
import task_checks
import task_products
import task_workflows
import task_release
from smoke_vsix import smoke_vsix

def create_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        description='FlexiMark development commands. Use mise run <command> [arguments].',
        epilog='Every command supports --dry-run. Build/package require an explicit target. See docs/DEVELOPMENT.md.',
    )
    commands = parser.add_subparsers(dest='command', metavar='command')

    def command(name: str, help_text: str) -> argparse.ArgumentParser:
        subparser = commands.add_parser(name, help=help_text, description=help_text)
        subparser.add_argument(
            '--dry-run',
            action='store_true',
            help='print the ordered steps without executing them',
        )
        return subparser
    command(
        'setup',
        'Install locked dependencies, Rust components, WASM target and pinned submodules',
    )
    command(
        'watch',
        'Build only the VS Code product, then watch JS/TS; does not launch an editor or watch Rust',
    )
    command('build', 'Build the selected product; no tests or editor launch').add_argument(
        'target',
        choices=('core', 'runtime', 'vscode', 'zed', 'all'),
    )
    command(
        'check',
        'Check generated files, architecture, formatting, types and lint; no test suites',
    )
    command('test', 'Run tests; default is unit (Python + Rust + Node, without GUI)').add_argument(
        'suite',
        nargs='?',
        default='unit',
        choices=('unit', 'editor'),
        help='editor builds the VS Code product and launches integration test hosts',
    )
    command(
        'package',
        'Build and package the selected product; no tests, smoke test or publication',
    ).add_argument(
        'target',
        choices=('runtime', 'vscode', 'zed', 'all'),
    )
    command(
        'verify',
        'Run all checks, unit tests, network audit, all builds, performance/editor tests, VSIX packaging and clean-install smoke',
    )
    command(
        'audit',
        'Query the package registry for moderate-or-higher JavaScript dependency vulnerabilities (network)',
    )
    command('smoke', 'Install and test an existing VSIX in an isolated VS Code profile').add_argument(
        'vsix',
        type=Path,
    )
    command('generate', 'Update the selected tracked generated files').add_argument(
        'target',
        choices=('protocol', 'l10n', 'versions', 'architecture', 'all'),
    )
    command(
        'clean',
        'Delete generated JS/TS and test output in .build; preserve target and .artifacts',
    )
    command('ci', 'Internal CI phases; uses the same implementation as local commands').add_argument(
        'phase',
        choices=('quality', 'product', 'zed', 'matrix'),
    )
    task_release.register(command)
    return parser

def create_plan(args: argparse.Namespace) -> task_plan.Plan:
    extra = task_release.plan(args)
    if extra is not None:
        return extra
    command = args.command
    artifacts = f".artifacts/{metadata()['version']}/{host_target()}"
    if command == 'setup':
        return task_plan.Plan(
            [
                task_plan.Step(
                    'Initialize/update pinned Git submodules',
                    lambda: run('git', 'submodule', 'update', '--init', '--recursive'),
                ),
                task_plan.Step(
                    'Install locked JavaScript dependencies',
                    lambda: run('yarn', 'install', '--immutable'),
                ),
                task_plan.Step(
                    'Install Rust formatting and lint components',
                    lambda: run('rustup', 'component', 'add', 'rustfmt', 'clippy'),
                ),
                task_plan.Step(
                    'Install WASI target for Zed',
                    lambda: run('rustup', 'target', 'add', 'wasm32-wasip2'),
                ),
                # Cold runners need all-target dependencies cached before offline architecture checks.
                task_plan.Step(
                    'Cache locked root Rust dependencies for offline architecture checks',
                    lambda: run('cargo', 'fetch', '--locked'),
                ),
                task_plan.Step(
                    'Cache locked Zed Rust dependencies for offline architecture checks',
                    lambda: run('cargo', 'fetch', '--locked', '--manifest-path', 'adapters/zed/Cargo.toml'),
                ),
            ],
            'Development dependencies ready. Toolchain versions are installed first with mise install.',
        )
    if command in {'build', 'package'}:
        steps = task_workflows.build_steps(args.target)
        if command == 'package' and args.target in {'vscode', 'all'}:
            steps.append(
                task_plan.Step(
                    'Create platform VSIX and verify its source identity',
                    lambda: task_products.package_identified_product(()),
                ),
            )
        runtime_zip = f"{artifacts}/fleximark-{metadata()['version']}-{host_target()}.zip"
        zed_zip = f"{artifacts}/fleximark-zed-{metadata()['version']}.zip"
        vsix = f"{artifacts}/fleximark-{metadata()['version']}-{host_target()}.vsix"
        result = {
            'core': 'Rust libraries in target/debug/',
            'runtime': f'{artifacts}/runtime/ and {runtime_zip} (+ .sha256)',
            'vscode': f'{artifacts}/vscode/ and {runtime_zip} (+ .sha256). To create a VSIX: mise run package vscode',
            'zed': f'{artifacts}/zed/, {zed_zip} and {runtime_zip} (+ .sha256)',
            'all': f'{artifacts}/runtime/, vscode/, zed/ and runtime/Zed ZIPs (+ .sha256). To create a VSIX too: mise run package all',
        }[args.target]
        if command == 'package' and args.target in {'vscode', 'all'}:
            result = vsix if args.target == 'vscode' else f'{vsix}, {runtime_zip} and {zed_zip} (+ .sha256 for ZIPs)'
        return task_plan.Plan(steps, result)
    if command == 'watch':
        return task_plan.Plan(
            [
                *task_workflows.build_steps('vscode'),
                task_plan.Step(
                    'Copy VS Code staging to .build/vscode/product; watch JavaScript/TypeScript until Ctrl+C (no editor launch)',
                    task_watch.watch_vscode,
                ),
            ],
            '.build/vscode/product; F5 launches the extension. Rust/CSS changes require stopping watch and restarting F5.',
        )
    if command == 'check':
        return task_plan.Plan(
            task_workflows.check_steps(),
            'Source checks passed. Stale generated files are regenerated and reported as a failure.',
        )
    if command == 'test':
        if args.suite == 'editor':
            return task_plan.Plan(
                [
                    *task_workflows.build_steps('vscode'),
                    task_plan.Step('Compile editor integration suites', task_js.compile_tests),
                    task_plan.Step(
                        'Run VS Code workspace, standalone and restricted-workspace integration tests',
                        task_js.run_editor_integration,
                    ),
                ],
                'VS Code integration tests passed (GUI; may download VS Code).',
            )
        return task_plan.Plan(
            task_workflows.unit_steps(),
            'All unit/native tests passed; no editor/browser launched.',
        )
    if command == 'verify':
        return task_plan.Plan(
            [
                task_plan.Step(
                    'Enable real browser acceptance tests unless explicitly configured',
                    lambda: os.environ.setdefault('FLEXIMARK_BROWSER_DOM', 'true'),
                ),
                *task_workflows.check_steps(),
                *task_workflows.unit_steps(check_contract=False),
                task_plan.Step('Audit JavaScript dependencies (network)', lambda: task_checks.verify_audit(())),
                *task_workflows.build_steps('all', check_contract=False),
                task_plan.Step(
                    'Check performance, run VS Code integration, create VSIX, verify identity and smoke-test clean installation',
                    task_products.verify_built_product,
                ),
            ],
            f'All local release gates passed; packages in {artifacts}. Requires GUI/browser and network; publishes nothing.',
        )
    if command == 'audit':
        return task_plan.Plan(
            [
                task_plan.Step(
                    'Run yarn npm audit for moderate-or-higher vulnerabilities',
                    lambda: task_checks.verify_audit(()),
                ),
            ],
            'Dependency audit passed.',
        )
    if command == 'smoke':
        return task_plan.Plan(
            [task_plan.Step(f'Clean-install and smoke-test {args.vsix}', lambda: smoke_vsix(args.vsix))],
            'Existing VSIX passed clean-install smoke test (GUI).',
        )
    if command == 'generate':
        generators = {
            'protocol': task_plan.Step(
                'Regenerate protocol schema, TypeScript contract and fixtures',
                generate_protocol_contract,
            ),
            'l10n': task_plan.Step('Regenerate English localization bundle', export_localization),
            'versions': task_plan.Step('Synchronize product manifests from release.toml', generate_versions),
            'architecture': task_plan.Step(
                'Check all Rust architectures and regenerate diagrams',
                task_checks.check_architecture,
            ),
        }
        steps = list(generators.values()) if args.target == 'all' else [generators[args.target]]
        return task_plan.Plan(
            steps,
            'Generated files updated; review and commit their changes with the source.',
        )
    if command == 'clean':
        return task_plan.Plan(
            [
                task_plan.Step(
                    'Remove .build/vscode, preview, workspace-ui, types and tests',
                    lambda: task_js.clean(()),
                ),
            ],
            'JS/TS and test output removed. Rust target and distribution artifacts preserved.',
        )
    if args.command == 'ci' and args.phase == 'matrix':
        return task_plan.Plan(
            [task_plan.Step('Write CI distribution matrix', task_checks.write_ci_matrix)],
            'Distribution matrix ready.',
        )
    phases = {
        'quality': [*task_workflows.check_steps(), *task_workflows.unit_steps(check_contract=False)],
        'product': [
            *task_workflows.build_steps('vscode'),
            task_plan.Step(
                'Check performance, run editor integration, create VSIX, verify identity and smoke-test clean installation',
                task_products.verify_built_product,
            ),
        ],
        'zed': [
            task_plan.Step('Synchronize product manifests from release.toml', generate_versions),
            task_plan.Step(
                'Build only Zed WASM extension and create ZIP/checksum',
                task_products.build_zed_product,
            ),
        ],
    }
    return task_plan.Plan(phases[args.phase], f'CI {args.phase} phase passed.')

def main(argv: Sequence[str] | None=None) -> None:
    parser = create_parser()
    args = parser.parse_args(argv)
    if args.command is None:
        parser.print_help()
        return
    create_plan(args).execute(dry_run=args.dry_run)
if __name__ == '__main__':
    script_entrypoint(main)
