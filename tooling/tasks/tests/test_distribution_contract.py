from __future__ import annotations

import json
import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import distribution_contract as contract


class DistributionContractTests(unittest.TestCase):
    def manifest(self, target: str) -> dict:
        return {
            "schemaVersion": 1,
            "protocolVersion": contract.metadata()["protocol_version"],
            "version": contract.metadata()["version"],
            "target": target,
            "files": {path: "a" * 64 for path in contract.runtime_paths(target)},
        }

    def test_each_target_uses_the_exact_policy_payload_set(self):
        for target in contract.TARGETS.values():
            value = self.manifest(target)
            self.assertEqual(contract.read_manifest(json.dumps(value).encode(), "fixture"), value)
            value["files"]["unlisted.js"] = "a" * 64
            with self.assertRaisesRegex(RuntimeError, "canonical runtime files"):
                contract.read_manifest(json.dumps(value).encode(), "fixture")

    def test_versions_reject_booleans_and_old_values(self):
        target = next(iter(contract.TARGETS.values()))
        for field in ("schemaVersion", "protocolVersion"):
            for invalid in (True, False, 0, "1"):
                value = self.manifest(target)
                value[field] = invalid
                with self.subTest(field=field, invalid=invalid), self.assertRaises(RuntimeError):
                    contract.read_manifest(json.dumps(value).encode(), "fixture")

    def test_duplicate_fields_and_noncanonical_hashes_are_rejected(self):
        target = next(iter(contract.TARGETS.values()))
        value = self.manifest(target)
        encoded = json.dumps(value).replace('"schemaVersion": 1', '"schemaVersion": 1, "schemaVersion": 1')
        with self.assertRaisesRegex(RuntimeError, "repeated field"):
            contract.read_manifest(encoded.encode(), "fixture")
        value["files"][next(iter(value["files"]))] = "A" * 64
        with self.assertRaisesRegex(RuntimeError, "SHA-256"):
            contract.read_manifest(json.dumps(value).encode(), "fixture")
