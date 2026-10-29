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

from _tools import ROOT, executable, format_process_error, run, script_entrypoint


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


def benchmark_executable(messages: str) -> Path:
    candidates: set[Path] = set()
    for line in messages.splitlines():
        try:
            message = json.loads(line)
        except json.JSONDecodeError as error:
            raise RuntimeError("invalid Cargo benchmark artifact JSON") from error
        if not isinstance(message, dict) or message.get("reason") != "compiler-artifact":
            continue
        target = message.get("target", {})
        if not isinstance(target, dict):
            continue
        if target.get("name") != "document_pipeline" or "bench" not in target.get("kind", []):
            continue
        manifest = message.get("manifest_path")
        if not isinstance(manifest, str) or Path(manifest) != ROOT / "apps" / "cli" / "Cargo.toml":
            continue
        path = message.get("executable")
        if isinstance(path, str) and path:
            candidates.add(Path(path))
    if len(candidates) != 1:
        raise RuntimeError(f"expected one document_pipeline benchmark executable, found {len(candidates)}")
    return next(iter(candidates))


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
        (ROOT / "contracts" / "capabilities" / "performance-budgets.json").read_text(
            encoding="utf-8"
        )
    )
    multiplier = float(os.environ.get("FLEXIMARK_PERF_MULTIPLIER", "1"))
    if not math.isfinite(multiplier) or not 1 <= multiplier <= 4:
        raise RuntimeError("FLEXIMARK_PERF_MULTIPLIER must be between 1 and 4")

    from distribution import host_target
    binary = ROOT / "target" / host_target() / "release" / (
        "fleximark.exe" if sys.platform == "win32" else "fleximark"
    )
    if not binary.is_file():
        run("cargo", "build", "--release", "-p", "fleximark-cli", "--target", host_target(), "--locked")

    with tempfile.TemporaryDirectory(prefix="fleximark-perf-") as temp:
        cold_start_ms = render(binary, Path(temp), "cold", "# Cold\n")

    build = checked_process(
        [executable("cargo"), "bench", "--no-run", "--message-format=json", "-p", "fleximark-cli", "--bench", "document_pipeline", "--locked"],
        timeout=900,
        description="benchmark compilation",
    )
    if build.stderr:
        print(build.stderr, end="", file=sys.stderr, flush=True)
    benchmark_path = benchmark_executable(build.stdout)
    if not benchmark_path.is_file():
        raise RuntimeError(f"benchmark executable does not exist: {benchmark_path}")
    benchmark = checked_process(
        [str(benchmark_path), "--json"],
        timeout=60,
        description="benchmark",
    )
    if benchmark.stderr:
        print(benchmark.stderr, end="", file=sys.stderr, flush=True)
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
    print(json.dumps({"schemaVersion": 1, "multiplier": multiplier, "metrics": metrics}), flush=True)
    for name, measured in metrics.items():
        allowed = contract["budgets"][name] * multiplier
        if measured > allowed:
            raise RuntimeError(f"{name} {measured} exceeds {allowed}")



if __name__ == "__main__":
    script_entrypoint(check_performance_budgets)
