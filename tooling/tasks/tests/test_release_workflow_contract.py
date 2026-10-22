from __future__ import annotations

import re
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[3]


class ReleaseWorkflowContractTests(unittest.TestCase):
    def source(self, name):
        return (ROOT / ".github" / "workflows" / name).read_text(encoding="utf-8")

    def test_all_native_targets_are_packaged_once_and_smoked_on_matching_hosts(self):
        for name in ("ci.yml", "release.yml"):
            source = self.source(name)
            rows = re.findall(r"target:\s*([a-z0-9_-]+),\s*vscode:\s*([a-z0-9-]+)", source)
            self.assertEqual(len(rows), 6)
            self.assertEqual(len(set(rows)), 6)
            self.assertIn("tasks.py verify-product", source)
            self.assertIn("xvfb-run -a python tooling/tasks/tasks.py verify-product", source)
            self.assertIn("overwrite: true", source)

    def test_all_checkouts_include_the_pinned_quality_submodule(self):
        for path in (ROOT / ".github" / "workflows").glob("*.yml"):
            source = path.read_text(encoding="utf-8")
            self.assertEqual(source.count("actions/checkout@"), source.count("submodules: recursive"), path)

    def test_rust_quality_covers_every_owned_workspace_and_checks_fresh_diagrams(self):
        for name in ("ci.yml", "release.yml"):
            source = self.source(name)
            self.assertIn("tasks.py verify-quality", source)
            self.assertIn("tasks.py verify-audit", source)

    def test_publish_revalidates_all_exact_assets_before_attestation_and_publication(self):
        source = self.source("release.yml")
        verify = source.index("release_assets.py verify")
        attest = source.index("actions/attest@")
        upload = source.index('gh release upload "$RELEASE_REF"')
        publish = source.index('gh release edit "$RELEASE_REF" --draft=false')
        self.assertLess(verify, attest)
        self.assertLess(attest, upload)
        self.assertLess(upload, publish)
        self.assertIn("environment: release", source)
        self.assertIn("attestations: write", source)
        self.assertIn("--verify-tag --draft", source)
        self.assertRegex(source, r'''tags: \[["']v\*["']\]''')
        self.assertNotIn("--clobber", source)

    def test_untrusted_dynamic_identity_is_passed_through_environment(self):
        source = self.source("release.yml")
        self.assertIn('SOURCE_GIT_HEAD: ${{ github.sha }}', source)
        self.assertIn('RELEASE_REF: ${{ github.ref_name }}', source)
        for line in source.splitlines():
            if line.lstrip().startswith(("python ", "gh ")):
                self.assertNotIn("${{", line)


if __name__ == "__main__":
    unittest.main()
