from __future__ import annotations

import json
import unittest
from pathlib import Path

from test_protocol_contract import Draft202012FixtureValidator, SchemaViolation


ROOT = Path(__file__).resolve().parents[2]
SCHEMA = ROOT / "schemas" / "protocol.schema.json"
INVALID_FIXTURE = ROOT / "test" / "fixtures" / "protocol-v3-invalid.json"
RENDER_NAVIGATION_FIXTURE = (
    ROOT / "test" / "fixtures" / "protocol-v3-render-navigation.json"
)
OPTIONAL_PRESENT_FIXTURE = (
    ROOT / "test" / "fixtures" / "protocol-v3-optional-present.json"
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
        cls.validator = Draft202012FixtureValidator(cls.schema)

    def test_nested_and_envelope_boundary_fixture_is_rejected(self) -> None:
        self.assertEqual(self.fixture["schemaVersion"], 3)
        self.assertGreaterEqual(len(self.fixture["cases"]), 6)
        names = [case["name"] for case in self.fixture["cases"]]
        self.assertEqual(len(names), len(set(names)))

        for case in self.fixture["cases"]:
            with self.subTest(case=case["name"]):
                with self.assertRaises(SchemaViolation):
                    self.validator.validate(
                        case["value"], self.schema["$defs"][case["definition"]]
                    )

    def test_rust_selection_wire_matches_render_navigation_schema(self) -> None:
        """Phase 1 red test: Rust valid wire must also be valid schema input."""
        self.assertEqual(self.render_navigation_fixture["schemaVersion"], 3)
        for event in self.render_navigation_fixture["events"]:
            with self.subTest(activePosition=event.get("activePosition", "omitted")):
                self.validator.validate(
                    event, self.schema["$defs"]["renderNavigationEvent"]
                )

    def test_valid_optional_present_dtos_remain_accepted(self) -> None:
        self.assertEqual(self.optional_present_fixture["schemaVersion"], 3)
        for case in self.optional_present_fixture["cases"]:
            with self.subTest(case=case["name"]):
                self.validator.validate(
                    case["value"], self.schema["$defs"][case["definition"]]
                )


if __name__ == "__main__":
    unittest.main()
