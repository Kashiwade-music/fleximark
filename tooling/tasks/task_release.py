"""Register distribution and release operations on the common task CLI."""
from __future__ import annotations
import argparse
import os
from pathlib import Path
import build
import distribution
import protocol_codegen
import release_artifact
import release_assets
from task_plan import Plan, Step

def register(command) -> None:
    bundle = command('bundle', 'Build one JavaScript bundle group')
    bundle.add_argument('target', choices=('browser', 'workspace', 'extension', 'tests'))
    bundle.add_argument('--production', action='store_true')
    contract = command('contract', 'Generate or check the current protocol contract')
    contract.add_argument('mode', choices=('generate', 'check'))
    product = command('distribution', 'Assemble distribution inputs for an explicit target')
    product.add_argument('operation', choices=('versions', 'native', 'runtime', 'vscode'))
    product.add_argument('--target', default=distribution.host_target())
    product.add_argument('--native-dir', type=Path)
    product.add_argument('--archive', type=Path)
    assets = command('release-assets', 'Verify every release artifact and source identity')
    assets.add_argument('--root', type=Path, required=True)
    assets.add_argument('--source-git-head', required=True)
    assets.add_argument('--tag', required=True)
    artifact = command('artifact', 'Create or verify a VSIX identity, or validate a VSIX')
    artifact.add_argument('operation', choices=('create', 'verify', 'validate'))
    artifact.add_argument('--vsix', type=Path, default=release_artifact.DEFAULT_VSIX)
    artifact.add_argument('--identity', type=Path, default=release_artifact.DEFAULT_IDENTITY)
    artifact.add_argument('--expected-version')
    artifact.add_argument('--git-tag')
    artifact.add_argument('--source-git-head')
    artifact.add_argument('--expected-sha256', default=os.environ.get('FLEXIMARK_EXPECTED_SHA256'))
    artifact.add_argument(
        '--expected-git-tag',
        default=os.environ.get('FLEXIMARK_EXPECTED_GIT_TAG'),
    )
    artifact.add_argument(
        '--expected-source-git-head',
        default=os.environ.get('FLEXIMARK_EXPECTED_SOURCE_GIT_HEAD'),
    )
    artifact.add_argument('--print-sha256', action='store_true')

def artifact(args: argparse.Namespace) -> None:
    if args.operation == 'validate':
        release_artifact.validate_vsix(args.vsix, expected_version=args.expected_version)
        return
    if args.operation == 'create':
        if not args.expected_version or not args.git_tag or (not args.source_git_head):
            raise RuntimeError('create requires version, git tag, and source git head')
        identity = release_artifact.create_identity(
            args.vsix,
            args.identity,
            expected_version=args.expected_version,
            git_tag=args.git_tag,
            source_git_head=args.source_git_head,
        )
    else:
        identity = release_artifact.verify_identity(
            args.vsix,
            args.identity,
            expected_sha256=args.expected_sha256,
            expected_git_tag=args.expected_git_tag,
            expected_source_git_head=args.expected_source_git_head,
        )
    if args.print_sha256:
        print(identity['sha256'])

def assemble(args: argparse.Namespace) -> None:
    if args.operation == 'versions':
        distribution.generate_versions()
    elif args.operation == 'native':
        distribution.generate_versions()
        distribution.build_native(args.target)
    elif args.operation == 'runtime':
        print(distribution.assemble_runtime(args.target, native_dir=args.native_dir))
    else:
        if args.archive is None:
            raise RuntimeError('distribution vscode requires --archive')
        print(distribution.assemble_vscode(args.target, args.archive))

def plan(args: argparse.Namespace) -> Plan | None:
    if args.command == 'artifact':
        return Plan([Step(f'{args.operation} VSIX identity', lambda: artifact(args))], 'Artifact verified.')
    if args.command == 'release-assets':
        return Plan(
            [
                Step(
                    'Verify complete release assets',
                    lambda: release_assets.verify(args.root, args.source_git_head, args.tag),
                ),
            ],
            'Release assets verified.',
        )
    if args.command == 'distribution':
        return Plan(
            [Step(f'{args.operation} distribution for {args.target}', lambda: assemble(args))],
            'Distribution operation complete.',
        )
    if args.command == 'contract':
        return Plan(
            [Step(f'{args.mode} protocol artifacts', lambda: protocol_codegen.run_codegen(args.mode))],
            'Contract operation complete.',
        )
    if args.command == 'bundle':
        action = {
            'browser': build.build_browser_client,
            'workspace': build.build_workspace_ui,
            'extension': lambda: build.build_extension(production=args.production),
            'tests': build.build_tests,
        }[args.target]
        return Plan(
            [
                Step('Check generated protocol', protocol_codegen.check_protocol_contract),
                Step(f'Build {args.target} bundles', action),
            ],
            'Bundles ready.',
        )
    return None
