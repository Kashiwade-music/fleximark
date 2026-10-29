from __future__ import annotations

import sys
import unittest
from pathlib import Path
from unittest.mock import patch, call


ROOT = Path(__file__).resolve().parents[3]
SCRIPTS = ROOT / "tooling/tasks"
if str(SCRIPTS) not in sys.path:
    sys.path.insert(0, str(SCRIPTS))

import protocol_codegen
import tasks
import build as javascript_build


class ProtocolCodegenTests(unittest.TestCase):
    @patch("protocol_codegen.run")
    def test_modes_use_the_locked_workspace_generator(self, run_mock) -> None:
        for mode, task in (
            ("generate", protocol_codegen.generate_protocol_contract),
            ("check", protocol_codegen.check_protocol_contract),
        ):
            with self.subTest(mode=mode):
                run_mock.reset_mock()
                task()
                self.assertEqual(run_mock.call_args_list, [
                    call("cargo", "run", "--locked", "-p", "fleximark-protocol-codegen", "--", mode),
                    call("cargo", "xtask", "fixtures", mode),
                ])

    def test_invalid_public_and_internal_inputs_are_rejected(self) -> None:
        with self.assertRaises(RuntimeError):
            protocol_codegen.generate_protocol_contract(("unexpected",))
        with self.assertRaises(RuntimeError):
            protocol_codegen.check_protocol_contract(("unexpected",))
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
                    protocol_codegen,
                    "check_protocol_contract",
                    side_effect=lambda: events.append("check"),
                ),
                patch.object(
                    javascript_build,
                    builder,
                    side_effect=lambda *args, **kwargs: events.append("build"),
                ),
            ):
                tasks.main(["bundle", target])
            self.assertEqual(events, ["check", "build"])


if __name__ == "__main__":
    unittest.main()
