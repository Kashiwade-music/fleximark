from __future__ import annotations

import sys
import unittest
from pathlib import Path
from unittest.mock import patch


ROOT = Path(__file__).resolve().parents[2]
SCRIPTS = ROOT / "scripts"
if str(SCRIPTS) not in sys.path:
    sys.path.insert(0, str(SCRIPTS))

import protocol_codegen
import build as javascript_build


class ProtocolCodegenTests(unittest.TestCase):
    @patch("protocol_codegen.run")
    def test_generate_uses_locked_workspace_generator(self, run_mock) -> None:
        protocol_codegen.generate_protocol_contract()
        run_mock.assert_called_once_with(
            "cargo",
            "run",
            "--locked",
            "-p",
            "fleximark-protocol-codegen",
            "--",
            "generate",
        )

    @patch("protocol_codegen.run")
    def test_check_uses_locked_workspace_generator(self, run_mock) -> None:
        protocol_codegen.check_protocol_contract()
        run_mock.assert_called_once_with(
            "cargo",
            "run",
            "--locked",
            "-p",
            "fleximark-protocol-codegen",
            "--",
            "check",
        )

    def test_public_tasks_reject_arguments(self) -> None:
        with self.assertRaises(RuntimeError):
            protocol_codegen.generate_protocol_contract(("unexpected",))
        with self.assertRaises(RuntimeError):
            protocol_codegen.check_protocol_contract(("unexpected",))

    def test_unknown_internal_mode_is_rejected_before_spawning_cargo(self) -> None:
        with self.assertRaises(ValueError):
            protocol_codegen.run_codegen("unknown")

    def test_direct_javascript_builds_check_generated_contract_first(self) -> None:
        for target, builder in (
            ("browser", "build_browser_client"),
            ("extension", "build_extension"),
            ("tests", "build_tests"),
        ):
            events: list[str] = []
            with (
                self.subTest(target=target),
                patch.object(sys, "argv", ["build.py", target]),
                patch.object(
                    javascript_build,
                    "check_protocol_contract",
                    side_effect=lambda: events.append("check"),
                ),
                patch.object(
                    javascript_build,
                    builder,
                    side_effect=lambda *args, **kwargs: events.append("build"),
                ),
            ):
                javascript_build.main()
            self.assertEqual(events, ["check", "build"])


if __name__ == "__main__":
    unittest.main()
