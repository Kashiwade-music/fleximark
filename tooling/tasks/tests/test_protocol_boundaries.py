from __future__ import annotations

import json
import unittest
from pathlib import Path



ROOT = Path(__file__).resolve().parents[3]
SCHEMA = ROOT / "contracts" / "schemas" / "protocol.schema.json"
INVALID_FIXTURE = ROOT / "tests" / "fixtures" / "protocol" / "invalid.json"
RENDER_NAVIGATION_FIXTURE = (
    ROOT / "tests" / "fixtures" / "protocol" / "render-navigation.json"
)
OPTIONAL_PRESENT_FIXTURE = (
    ROOT / "tests" / "fixtures" / "protocol" / "optional-present.json"
)


class ProtocolBoundaryTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        cls.schema = json.loads(SCHEMA.read_text(encoding="utf-8"))
        cls.fixture = json.loads(INVALID_FIXTURE.read_text(encoding="utf-8"))
        cls.render_navigation_fixture = json.loads(
            RENDER_NAVIGATION_FIXTURE.read_text(encoding="utf-8")
        )
        cls.optional_present_fixture = json.loads(
            OPTIONAL_PRESENT_FIXTURE.read_text(encoding="utf-8")
        )

    def test_nested_and_envelope_boundary_fixture_is_rejected(self) -> None:
        import distribution
        self.assertEqual(self.fixture["schemaVersion"], distribution.metadata()["protocol_version"])
        self.assertGreaterEqual(len(self.fixture["cases"]), 6)
        names = [case["name"] for case in self.fixture["cases"]]
        self.assertEqual(len(names), len(set(names)))

        # Validation runs in tests/typescript/protocol-contract.test.mts.
        for fixture in (self.render_navigation_fixture, self.optional_present_fixture):
            self.assertEqual(fixture["schemaVersion"], self.fixture["schemaVersion"])

if __name__ == "__main__":
    unittest.main()
