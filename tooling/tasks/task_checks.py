from __future__ import annotations
import os
import json
from pathlib import Path
from collections.abc import Callable, Sequence
from _tools import ROOT, run
from distribution_contract import SPEC
from l10n_export import export_localization

def check_localization() -> None:
    check_generated_freshness(export_localization, [ROOT / 'adapters/vscode/l10n'])

def check_generated_freshness(generate: Callable[[], None], paths: Sequence[Path]) -> None:
    """Check regeneration against the working tree, including added/deleted files."""

    def snapshot() -> dict[Path, bytes]:
        files: dict[Path, bytes] = {}
        for path in paths:
            candidates = path.rglob('*') if path.is_dir() else [path]
            files.update(
                ((candidate, candidate.read_bytes()) for candidate in candidates if candidate.is_file()),
            )
        return files
    before = snapshot()
    generate()
    after = snapshot()
    changed = sorted(
        (str(path.relative_to(ROOT) if path.is_relative_to(ROOT) else path) for path in before.keys() | after.keys() if before.get(path) != after.get(path)),
    )
    if changed:
        raise RuntimeError('generated files were stale; review regenerated output: ' + ', '.join(changed))

def check_architecture() -> None:
    run('cargo', 'xtask', 'check')
    run('cargo', 'xtask', 'check', '--root', 'adapters/zed')

def verify_audit(_: Sequence[str]) -> None:
    """Network dependency audit required explicitly by CI and release workflows."""
    run('yarn', 'npm', 'audit', '--all', '--recursive', '--severity', 'high')

def write_ci_matrix() -> None:
    value = json.dumps(
        {
            'include': [{'target': target['triple'], 'os': target['runner']} for target in SPEC['targets']],
        },
        separators=(',', ':'),
    )
    output = os.environ.get('GITHUB_OUTPUT')
    if output:
        with Path(output).open('a', encoding='utf-8') as stream:
            stream.write(f'targets={value}\n')
    else:
        print(value)
