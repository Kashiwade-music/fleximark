from __future__ import annotations
import ast
import hashlib
import json
import re
import subprocess
import sys
import unittest
from pathlib import Path
ROOT = Path(__file__).resolve().parents[3]
SCRIPTS = ROOT / 'tooling/tasks'
if str(SCRIPTS) not in sys.path:
    sys.path.insert(0, str(SCRIPTS))

def read(relative: str) -> str:
    return (ROOT / relative).read_text(encoding='utf-8')

def run(*command: str) -> subprocess.CompletedProcess[str]:
    result = subprocess.run(
        command,
        cwd=ROOT,
        capture_output=True,
        encoding='utf-8',
        errors='replace',
        check=False,
    )
    if result.returncode != 0:
        raise AssertionError(
            f"command failed ({result.returncode}): {' '.join(command)}\nstdout:\n{result.stdout}\nstderr:\n{result.stderr}",
        )
    return result

def include_typescript_import_closure(paths: set[Path]) -> set[Path]:
    discovered = set(paths)
    pending = list(paths)
    import_pattern = re.compile('(?:from\\s+|import\\s*)["\\\']([^"\\\']+)["\\\']')
    while pending:
        owner = pending.pop()
        if not owner.is_file() or owner.suffix not in {'.mts', '.ts'}:
            continue
        for specifier in import_pattern.findall(owner.read_text(encoding='utf-8')):
            if not specifier.startswith('.'):
                continue
            imported = (owner.parent / specifier).resolve()
            candidates = [imported]
            if imported.suffix == '.mjs':
                candidates.append(imported.with_suffix('.mts'))
            elif imported.suffix == '.js':
                candidates.extend([imported.with_suffix('.ts'), imported.with_suffix('.mts')])
            elif not imported.suffix:
                candidates.extend([imported.with_suffix('.ts'), imported.with_suffix('.mts')])
            for candidate in candidates:
                if candidate.is_file() and candidate not in discovered:
                    discovered.add(candidate)
                    pending.append(candidate)
    return discovered

class TypeScriptTestBoundaryContract(unittest.TestCase):

    def test_pure_and_electron_entries_preserve_complete_unique_suite_membership(self) -> None:
        from build import discover_test_suites
        root = ROOT / 'tests/typescript'
        groups = discover_test_suites(root)
        expected = {path.resolve() for path in root.rglob('*.test.mts')
                    if 'export const suiteName' in path.read_text(encoding='utf-8')}
        registered = [path.resolve() for suites in groups.values() for path in suites]
        self.assertEqual(set(registered), expected, 'every exported suite must run')
        self.assertEqual(len(registered), len(set(registered)), 'suites must run exactly once')
        for host, suites in groups.items():
            self.assertTrue(suites, f'{host} must retain its test coverage')
            self.assertTrue(all(path.relative_to(root).parts[0] == host for path in suites))
        self.assertEqual([path.name for path in groups['standalone']], ['standalone-runtime.test.mts'])
        self.assertTrue({'export-ack.test.mts', 'workspace-selection.test.mts'} <=
                        {path.name for path in groups['unit']})

class JavaScriptEntrypointContract(unittest.TestCase):

    def test_typescript_projects_cover_every_production_and_test_source(self) -> None:
        configs = [ROOT / 'tsconfig.json']
        visited: set[Path] = set()
        covered: set[Path] = set()
        membership: dict[Path, int] = {}
        while configs:
            config_path = configs.pop().resolve()
            if config_path in visited:
                continue
            visited.add(config_path)
            config = json.loads(config_path.read_text(encoding='utf-8'))
            base = config_path.parent
            excluded = {path.resolve() for pattern in config.get('exclude', []) for path in base.glob(pattern)}
            for include in config.get('include', []):
                for path in base.glob(include):
                    resolved = path.resolve()
                    if resolved in excluded:
                        continue
                    covered.add(resolved)
                    membership[resolved] = membership.get(resolved, 0) + 1
            for reference in config.get('references', []):
                referenced = (base / reference['path']).resolve()
                configs.append(referenced / 'tsconfig.json' if referenced.is_dir() else referenced)
        covered = include_typescript_import_closure(covered)
        expected = {path.resolve() for owner in ('adapters/vscode', 'web', 'contracts/typescript', 'tests/typescript') for path in (ROOT / owner).rglob('*.mts')}
        self.assertTrue(expected)
        self.assertEqual(
            {path.relative_to(ROOT).as_posix(): membership.get(path, 0) for path in expected if membership.get(path, 0) != 1},
            {},
            'every TypeScript source must belong directly to exactly one leaf project',
        )
        self.assertEqual(
            expected - covered,
            set(),
            'root include or referenced TypeScript projects must cover every source and test',
        )

class CargoBoundaryContract(unittest.TestCase):

    def test_workspace_metadata_keeps_lock_and_declares_rust_requirements(self) -> None:
        lock_path = ROOT / 'Cargo.lock'
        before = hashlib.sha256(lock_path.read_bytes()).digest()
        metadata = json.loads(run('cargo', 'metadata', '--format-version', '1', '--locked').stdout)
        after = hashlib.sha256(lock_path.read_bytes()).digest()
        self.assertEqual(after, before)
        workspace_ids = set(metadata['workspace_members'])
        workspace_packages = [package for package in metadata['packages'] if package['id'] in workspace_ids]
        self.assertTrue(workspace_packages)
        self.assertTrue(all((package['rust_version'] is not None for package in workspace_packages)))
        self.assertTrue(all((package['edition'] == '2024' for package in workspace_packages)))
        self.assertIn('fleximark-model', {p['name'] for p in workspace_packages})
        self.assertIn('fleximark-protocol', {p['name'] for p in workspace_packages})
if __name__ == '__main__':
    unittest.main()
