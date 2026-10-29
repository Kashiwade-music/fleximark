from __future__ import annotations

import sys
import tempfile
import unittest
from pathlib import Path

SCRIPTS = Path(__file__).resolve().parents[1]
if str(SCRIPTS) not in sys.path:
    sys.path.insert(0, str(SCRIPTS))
import build


class TestDiscoveryTests(unittest.TestCase):
    def test_existing_host_membership_matches_all_exported_suites(self):
        groups = build.discover_test_suites()
        root = build.ROOT / "tests/typescript"
        for host in groups:
            expected = {path.resolve() for path in (root / host).rglob('*.test.mts')
                        if 'export const suiteName' in path.read_text(encoding='utf-8')}
            self.assertEqual({path.resolve() for path in groups[host]}, expected)
        registered = [path for paths in groups.values() for path in paths]
        self.assertEqual(len(registered), len(set(registered)))

    def test_new_exported_suite_is_found_without_registration(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            for host in ("unit", "editor", "standalone"):
                suite = root / host / "new.test.mts"
                suite.parent.mkdir()
                suite.write_text('export const suiteName = "New";\nexport function suite(): void {}\n')
            (root / "helper.test.mts").write_text('export function helper() {}\n')
            groups = build.discover_test_suites(root)
            for host in groups:
                self.assertEqual(groups[host], [root / host / "new.test.mts"])

    def test_static_entries_preserve_runner_registration_and_activation(self):
        destination = build.TEST_OUTPUT / "generated" / "unit.test.mts"
        suites = [build.ROOT / "tests/typescript/unit/new.test.mts"]
        pure = build.test_entry_source("unit", suites, destination)
        self.assertIn('import * as suite0 from "../../../tests/typescript/unit/new.test.mjs";', pure)
        self.assertIn('test: it', pure)
        self.assertIn('describe(suite0.suiteName, suite0.suite);', pure)
        self.assertNotIn('import(', pure)
        editor = build.test_entry_source("editor", suites, destination)
        self.assertIn('await extension.activate();', editor)
        self.assertIn('suite(suite0.suiteName, suite0.suite);', editor)
        standalone = build.test_entry_source("standalone", suites, destination)
        self.assertNotIn('extension.activate', standalone)

    def test_suite_outside_execution_host_is_rejected(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / 'misplaced.test.mts').write_text(
                'export const suiteName = "Misplaced";\nexport function suite(): void {}\n')
            with self.assertRaisesRegex(ValueError, 'host directory'):
                build.discover_test_suites(root)

    def test_build_and_watch_share_bundle_options(self):
        for factory in (build.extension_args, build.preview_args, build.browser_args, build.workspace_args):
            normal = factory(production=False)
            watch = factory(production=False, watch=True)
            self.assertEqual([arg for arg in normal if not arg.startswith('--outfile=')],
                             [arg for arg in watch if not arg.startswith('--outfile=') and arg != '--watch=forever'])


if __name__ == "__main__":
    unittest.main()
