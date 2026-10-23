from __future__ import annotations

import json
import unittest
from pathlib import Path
from typing import Any


ROOT = Path(__file__).resolve().parents[3]
FIXTURE = ROOT / "tests" / "fixtures" / "protocol" / "contract.json"
BIDIRECTIONAL_SERVER_FIXTURE = (
    ROOT / "tests" / "fixtures" / "protocol" / "bidirectional-server.json"
)
SCHEMA = ROOT / "contracts" / "schemas" / "protocol.schema.json"


def schema_method_params(
    schema: dict[str, Any], definition: str
) -> dict[str, dict[str, Any]]:
    methods: dict[str, dict[str, Any]] = {}
    for alternative in schema["$defs"][definition]["oneOf"]:
        properties = alternative["allOf"][1]["properties"]
        selector = properties["method"]
        names = [selector["const"]] if "const" in selector else selector["enum"]
        for name in names:
            if name in methods:
                raise AssertionError(f"duplicate method in schema: {name}")
            methods[name] = properties["params"]
    return methods


class ProtocolContractTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        cls.fixture = json.loads(FIXTURE.read_text(encoding="utf-8"))
        cls.bidirectional_server_fixture = json.loads(
            BIDIRECTIONAL_SERVER_FIXTURE.read_text(encoding="utf-8")
        )
        cls.schema = json.loads(SCHEMA.read_text(encoding="utf-8"))

    def test_fixture_method_set_matches_schema(self) -> None:
        # The Rust protocol tests own registry-to-fixture validation. This test
        # owns the next link in the contract chain: fixture-to-schema.
        cases = self.fixture["methods"]
        fixture_methods = [case["method"] for case in cases]
        self.assertTrue(fixture_methods)
        self.assertEqual(len(fixture_methods), len(set(fixture_methods)))

        request_params = schema_method_params(self.schema, "customRequest")
        notification_params = schema_method_params(self.schema, "customNotification")
        schema_methods = request_params.keys() | notification_params.keys()
        self.assertEqual(set(fixture_methods), schema_methods)

        fixture_kinds = {case["method"]: case["kind"] for case in cases}
        self.assertEqual(
            {method for method, kind in fixture_kinds.items() if kind == "request"},
            request_params.keys(),
        )
        self.assertEqual(
            {
                method
                for method, kind in fixture_kinds.items()
                if kind == "notification"
            },
            notification_params.keys(),
        )

    # Fixture acceptance and rejection run in the production TypeScript validator suite.

if __name__ == "__main__":
    unittest.main()
