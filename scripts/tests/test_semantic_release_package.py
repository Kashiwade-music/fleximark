from __future__ import annotations

import subprocess
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]


class SemanticReleasePackageTests(unittest.TestCase):
    def test_node_unit_suite(self) -> None:
        result = subprocess.run(
            ["node", "--test", "scripts/tests/semantic_release_package.test.mjs"],
            cwd=ROOT,
            capture_output=True,
            encoding="utf-8",
            errors="replace",
            check=False,
        )
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)


if __name__ == "__main__":
    unittest.main()
