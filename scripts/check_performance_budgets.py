from __future__ import annotations

import json
import math
import os
import subprocess
import sys
import tempfile
import time
from pathlib import Path
from typing import Any

from _tools import ROOT, format_process_error, run, script_entrypoint


def checked_process(
    command: list[str], *, timeout: float, description: str
) -> subprocess.CompletedProcess[str]:
    try:
        result = subprocess.run(
            command,
            cwd=ROOT,
            capture_output=True,
            encoding="utf-8",
            errors="replace",
            timeout=timeout,
            check=True,
        )
    except (subprocess.CalledProcessError, subprocess.TimeoutExpired) as error:
        raise RuntimeError(f"{description}: {format_process_error(error)}") from error
    return result


def render(binary: Path, directory: Path, name: str, source: str) -> float:
    document = directory / f"{name}.md"
    document.write_text(source, encoding="utf-8")
    started = time.perf_counter()
    checked_process(
        [str(binary), "render", str(document)],
        timeout=30,
        description=f"{name} render",
    )
    return (time.perf_counter() - started) * 1000


def check_performance_budgets() -> None:
    contract: dict[str, Any] = json.loads(
        (ROOT / "capabilities" / "performance-budgets.json").read_text(
            encoding="utf-8"
        )
    )
    multiplier = float(os.environ.get("FLEXIMARK_PERF_MULTIPLIER", "1"))
    if not math.isfinite(multiplier) or not 1 <= multiplier <= 4:
        raise RuntimeError("FLEXIMARK_PERF_MULTIPLIER must be between 1 and 4")

    binary = ROOT / "target" / "release" / (
        "fleximark.exe" if sys.platform == "win32" else "fleximark"
    )
    if not binary.is_file():
        run("cargo", "build", "--release", "-p", "fleximark-cli", "--locked")

    with tempfile.TemporaryDirectory(prefix="fleximark-perf-") as temp:
        cold_start_ms = render(binary, Path(temp), "cold", "# Cold\n")

    benchmark = checked_process(
        [str(binary), "benchmark", "--json"],
        timeout=60,
        description="benchmark",
    )
    report = json.loads(benchmark.stdout)
    if report.get("schemaVersion") != 1:
        raise RuntimeError("unsupported benchmark schema")
    documents = report.get("documents", [])
    if [document.get("lines") for document in documents] != [1000, 10000, 100000]:
        raise RuntimeError("the benchmark must exercise exact line counts")
    for document in documents:
        if document.get("editCount") != 20:
            raise RuntimeError("each benchmark document must run 20 edits")

    metrics = {
        "coldStartMs": cold_start_ms,
        "render1kMs": documents[0]["initialRenderMs"],
        "render10kMs": documents[1]["initialRenderMs"],
        "render100kMs": documents[2]["initialRenderMs"],
        "editBurstMs": max(document["editBurstMs"] for document in documents),
        "frameBytes": max(document["maxFrameBytes"] for document in documents),
        "peakMemoryBytes": report["peakMemoryBytes"],
        "specialRenderMs": report["specialRenderMs"],
    }
    for name, measured in metrics.items():
        allowed = contract["budgets"][name] * multiplier
        if measured > allowed:
            raise RuntimeError(f"{name} {measured} exceeds {allowed}")

    print(json.dumps({"schemaVersion": 1, "multiplier": multiplier, "metrics": metrics}))


if __name__ == "__main__":
    script_entrypoint(check_performance_budgets)
