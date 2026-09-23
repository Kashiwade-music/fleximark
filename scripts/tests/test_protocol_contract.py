from __future__ import annotations

import json
import re
import unittest
from pathlib import Path
from typing import Any


ROOT = Path(__file__).resolve().parents[2]
FIXTURE = ROOT / "test" / "fixtures" / "protocol-v5-contract.json"
BIDIRECTIONAL_SERVER_FIXTURE = (
    ROOT / "test" / "fixtures" / "protocol-v5-bidirectional-server.json"
)
SCHEMA = ROOT / "schemas" / "protocol.schema.json"


class SchemaViolation(AssertionError):
    pass


class Draft202012FixtureValidator:
    """Validate the small JSON Schema vocabulary used by protocol fixtures."""

    def __init__(self, document: dict[str, Any]) -> None:
        self.document = document

    def validate(
        self, value: Any, schema: dict[str, Any], path: str = "$"
    ) -> None:
        if "$ref" in schema:
            reference = schema["$ref"]
            if not reference.startswith("#/"):
                raise SchemaViolation(f"{path}: external ref is unsupported: {reference}")
            target: Any = self.document
            for component in reference[2:].split("/"):
                target = target[component.replace("~1", "/").replace("~0", "~")]
            self.validate(value, target, path)

        for subschema in schema.get("allOf", []):
            self.validate(value, subschema, path)
        if "anyOf" in schema:
            if not any(self._matches(value, item, path) for item in schema["anyOf"]):
                raise SchemaViolation(f"{path}: no anyOf branch matched")
        if "oneOf" in schema:
            matches = sum(
                self._matches(value, item, path) for item in schema["oneOf"]
            )
            if matches != 1:
                raise SchemaViolation(
                    f"{path}: expected one oneOf match, observed {matches}"
                )

        if "const" in schema and not self._json_equal(value, schema["const"]):
            raise SchemaViolation(f"{path}: expected const {schema['const']!r}")
        if "enum" in schema and not any(
            self._json_equal(value, candidate) for candidate in schema["enum"]
        ):
            raise SchemaViolation(f"{path}: value is not in enum")

        expected_type = schema.get("type")
        if expected_type is not None:
            alternatives = (
                expected_type if isinstance(expected_type, list) else [expected_type]
            )
            if not any(self._has_type(value, item) for item in alternatives):
                raise SchemaViolation(
                    f"{path}: expected type {expected_type!r}, got {type(value).__name__}"
                )

        if isinstance(value, dict):
            properties = schema.get("properties", {})
            missing = set(schema.get("required", [])) - value.keys()
            if missing:
                raise SchemaViolation(f"{path}: missing required {sorted(missing)}")
            extras = value.keys() - properties.keys()
            additional = schema.get("additionalProperties", True)
            if additional is False and extras:
                raise SchemaViolation(f"{path}: unexpected properties {sorted(extras)}")
            if isinstance(additional, dict):
                for name in extras:
                    self.validate(value[name], additional, f"{path}.{name}")
            for name in value.keys() & properties.keys():
                self.validate(value[name], properties[name], f"{path}.{name}")
            if "propertyNames" in schema:
                for name in value:
                    self.validate(name, schema["propertyNames"], f"{path}.{name}")

        if isinstance(value, list):
            if schema.get("uniqueItems"):
                encoded = [json.dumps(item, sort_keys=True) for item in value]
                if len(encoded) != len(set(encoded)):
                    raise SchemaViolation(f"{path}: array items are not unique")
            if "minItems" in schema and len(value) < schema["minItems"]:
                raise SchemaViolation(f"{path}: array is too short")
            if "items" in schema:
                for index, item in enumerate(value):
                    self.validate(item, schema["items"], f"{path}[{index}]")

        if isinstance(value, str):
            if "minLength" in schema and len(value) < schema["minLength"]:
                raise SchemaViolation(f"{path}: string is too short")
            if "maxLength" in schema and len(value) > schema["maxLength"]:
                raise SchemaViolation(f"{path}: string is too long")
            if "pattern" in schema and re.search(schema["pattern"], value) is None:
                raise SchemaViolation(f"{path}: string does not match pattern")

        if isinstance(value, (int, float)) and not isinstance(value, bool):
            if "minimum" in schema and value < schema["minimum"]:
                raise SchemaViolation(f"{path}: number is below minimum")
            if "maximum" in schema and value > schema["maximum"]:
                raise SchemaViolation(f"{path}: number is above maximum")

    def _matches(self, value: Any, schema: dict[str, Any], path: str) -> bool:
        try:
            self.validate(value, schema, path)
        except SchemaViolation:
            return False
        return True

    @staticmethod
    def _has_type(value: Any, expected: str) -> bool:
        return {
            "null": value is None,
            "boolean": isinstance(value, bool),
            "integer": isinstance(value, int) and not isinstance(value, bool),
            "number": isinstance(value, (int, float)) and not isinstance(value, bool),
            "string": isinstance(value, str),
            "array": isinstance(value, list),
            "object": isinstance(value, dict),
        }.get(expected, False)

    @classmethod
    def _json_equal(cls, left: Any, right: Any) -> bool:
        """Compare JSON values without Python's bool/int coercion."""
        if left is None or right is None:
            return left is None and right is None
        if isinstance(left, bool) or isinstance(right, bool):
            return isinstance(left, bool) and isinstance(right, bool) and left == right
        if isinstance(left, (int, float)) or isinstance(right, (int, float)):
            return (
                isinstance(left, (int, float))
                and not isinstance(left, bool)
                and isinstance(right, (int, float))
                and not isinstance(right, bool)
                and left == right
            )
        if isinstance(left, str) or isinstance(right, str):
            return isinstance(left, str) and isinstance(right, str) and left == right
        if isinstance(left, list) or isinstance(right, list):
            return (
                isinstance(left, list)
                and isinstance(right, list)
                and len(left) == len(right)
                and all(cls._json_equal(a, b) for a, b in zip(left, right))
            )
        if isinstance(left, dict) or isinstance(right, dict):
            return (
                isinstance(left, dict)
                and isinstance(right, dict)
                and left.keys() == right.keys()
                and all(cls._json_equal(left[key], right[key]) for key in left)
            )
        return False


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
                raise SchemaViolation(f"duplicate method in schema: {name}")
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
        cls.validator = Draft202012FixtureValidator(cls.schema)

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

    def test_every_fixture_matches_its_params_and_result_schema(self) -> None:
        request_params = schema_method_params(self.schema, "customRequest")
        notification_params = schema_method_params(self.schema, "customNotification")
        result_schemas = self.schema["$defs"]["methodResults"]["properties"]
        fixture_methods = {
            case["method"] for case in self.fixture["methods"]
        }
        self.assertEqual(
            set(result_schemas),
            fixture_methods - {"fleximark/requestFullText"},
        )
        self.assertEqual(
            set(result_schemas) & notification_params.keys(),
            notification_params.keys() - {"fleximark/requestFullText"},
        )

        for case in self.fixture["methods"]:
            method = case["method"]
            with self.subTest(method=method):
                params_schema = (request_params | notification_params)[method]
                self.validator.validate(case["params"], params_schema)

                envelope = {
                    "jsonrpc": "2.0",
                    "method": method,
                    "params": case["params"],
                }
                if case["kind"] == "request":
                    envelope["id"] = 7
                    self.validator.validate(
                        envelope, self.schema["$defs"]["customRequest"]
                    )
                    self.validator.validate(
                        case["result"], result_schemas[method]
                    )
                    self.validator.validate(
                        {"jsonrpc": "2.0", "id": 7, "result": case["result"]},
                        self.schema["$defs"]["response"],
                    )
                else:
                    self.validator.validate(
                        envelope, self.schema["$defs"]["customNotification"]
                    )
                    self.assertIsNone(case["result"])
                    if method in result_schemas:
                        self.validator.validate(case["result"], result_schemas[method])

        # requestFullText is outbound-only and intentionally has no response slot.
        self.assertNotIn("fleximark/requestFullText", result_schemas)

    def test_bidirectional_methods_have_an_independent_server_fixture(self) -> None:
        bidirectional_methods = {
            case["method"]
            for case in self.fixture["methods"]
            if case["direction"] == "bidirectional"
        }
        cases = self.bidirectional_server_fixture["notifications"]
        self.assertEqual(
            {case["method"] for case in cases},
            bidirectional_methods,
        )
        server_params = schema_method_params(
            self.schema, "serverToClientCustomNotification"
        )
        for case in cases:
            method = case["method"]
            with self.subTest(method=method):
                self.validator.validate(case["params"], server_params[method])
                self.validator.validate(
                    {
                        "jsonrpc": "2.0",
                        "method": method,
                        "params": case["params"],
                    },
                    self.schema["$defs"]["serverToClientCustomNotification"],
                )

if __name__ == "__main__":
    unittest.main()
