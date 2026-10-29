from __future__ import annotations

import contextlib
import io
import json
import os
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

SCRIPTS = Path(__file__).resolve().parents[1]
if str(SCRIPTS) not in sys.path:
    sys.path.insert(0, str(SCRIPTS))
import check_performance_budgets as performance


def artifact(path: Path | None, *, name="document_pipeline", kind="bench", fresh=False):
    return {
        "reason": "compiler-artifact",
        "manifest_path": str(performance.ROOT / "apps/cli/Cargo.toml"),
        "target": {"name": name, "kind": [kind]},
        "executable": str(path) if path is not None else None,
        "fresh": fresh,
    }


def messages(*values):
    return "\n".join(json.dumps(value) for value in values)


class PerformanceBudgetTests(unittest.TestCase):
    def test_selects_exact_bench_from_mixed_artifacts_including_fresh(self):
        for fresh in (False, True):
            with self.subTest(fresh=fresh):
                expected = Path("target/release/deps/document_pipeline")
                wrong_package = artifact(Path("other/package/bench"))
                wrong_package["manifest_path"] = str(performance.ROOT / "other/Cargo.toml")
                output = messages(
                    {"reason": "build-script-executed"},
                    artifact(Path("lib"), kind="lib"),
                    artifact(Path("other"), name="other_bench"),
                    wrong_package, artifact(None),
                    artifact(expected, fresh=fresh), artifact(expected, fresh=fresh),
                    {"reason": "build-finished", "success": True},
                )
                self.assertEqual(performance.benchmark_executable(output), expected)

    def test_rejects_absent_wrong_or_ambiguous_artifacts(self):
        for output in (
            "", messages(artifact(None)),
            messages(artifact(Path("other"), name="other_bench")),
            messages(artifact(Path("a")), artifact(Path("b"))), "not JSON",
        ):
            with self.subTest(output=output), self.assertRaises(RuntimeError):
                performance.benchmark_executable(output)

    def run_measurement(self, *, peak=100, existing=True):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            contract = root / "contracts/capabilities/performance-budgets.json"
            contract.parent.mkdir(parents=True)
            names = ["coldStartMs", "render1kMs", "render10kMs", "render100kMs",
                     "editBurstMs", "frameBytes", "peakMemoryBytes", "specialRenderMs"]
            contract.write_text(json.dumps({"budgets": {name: 1000 for name in names}}), encoding="utf-8")
            binary = root / "target/host/release" / ("fleximark.exe" if sys.platform == "win32" else "fleximark")
            binary.parent.mkdir(parents=True)
            binary.touch()
            bench = root / "measured-bench"
            if existing:
                bench.touch()
            report = {"schemaVersion": 1, "peakMemoryBytes": peak, "specialRenderMs": 1,
                      "documents": [{"lines": lines, "editCount": 20, "initialRenderMs": 1,
                                     "editBurstMs": 1, "maxFrameBytes": 1}
                                    for lines in [1000, 10000, 100000]]}
            with patch.object(performance, "ROOT", root), patch("distribution.host_target", return_value="host"), patch.object(performance, "render", return_value=1), patch.object(performance, "executable", return_value="cargo-tool"), patch.dict(os.environ, {"FLEXIMARK_PERF_MULTIPLIER": "1"}), patch.object(performance, "checked_process") as process:
                process.side_effect = [
                    subprocess.CompletedProcess([], 0, messages(artifact(bench, fresh=True)), "compiled\n"),
                    subprocess.CompletedProcess([], 0, json.dumps(report), "benchmark phases\n"),
                ]
                output, diagnostics = io.StringIO(), io.StringIO()
                error = None
                with contextlib.redirect_stdout(output), contextlib.redirect_stderr(diagnostics):
                    try:
                        performance.check_performance_budgets()
                    except RuntimeError as caught:
                        error = caught
                return process.call_args_list, output.getvalue(), diagnostics.getvalue(), error, bench

    def test_executes_artifact_directly_and_preserves_compile_diagnostics(self):
        calls, output, diagnostics, error, bench = self.run_measurement()
        self.assertIsNone(error)
        self.assertEqual(calls[0].args[0], ["cargo-tool", "bench", "--no-run", "--message-format=json", "-p", "fleximark-cli", "--bench", "document_pipeline", "--locked"])
        self.assertEqual(calls[0].kwargs["timeout"], 900)
        self.assertEqual(calls[1].args[0], [str(bench), "--json"])
        self.assertEqual(calls[1].kwargs["timeout"], 60)
        self.assertEqual(diagnostics, "compiled\nbenchmark phases\n")
        self.assertEqual(json.loads(output)["metrics"]["peakMemoryBytes"], 100)

    def test_missing_executable_is_rejected_before_measurement(self):
        calls, _, _, error, _ = self.run_measurement(existing=False)
        self.assertEqual(len(calls), 1)
        self.assertIn("benchmark executable does not exist", str(error))

    def test_budget_excess_still_fails_after_emitting_all_metrics(self):
        _, output, _, error, _ = self.run_measurement(peak=1001)
        self.assertIn("peakMemoryBytes 1001 exceeds 1000.0", str(error))
        self.assertEqual(len(json.loads(output)["metrics"]), 8)
        self.assertEqual(json.loads(output)["metrics"]["peakMemoryBytes"], 1001)


if __name__ == "__main__":
    unittest.main()
