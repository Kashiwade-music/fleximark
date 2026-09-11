import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { performance } from "node:perf_hooks";
import process from "node:process";
import { URL, fileURLToPath } from "node:url";

const root = new URL("..", import.meta.url);
const contract = JSON.parse(
  fs.readFileSync(
    new URL("../capabilities/performance-budgets.json", import.meta.url),
  ),
);
const multiplier = Number(process.env.FLEXIMARK_PERF_MULTIPLIER ?? "1");
assert.ok(Number.isFinite(multiplier) && multiplier >= 1 && multiplier <= 4);
const binary = fileURLToPath(
  new URL(
    process.platform === "win32"
      ? "../target/release/fleximark.exe"
      : "../target/release/fleximark",
    import.meta.url,
  ),
);
if (!fs.existsSync(binary)) {
  const build = spawnSync(
    "cargo",
    ["build", "--release", "-p", "fleximark-cli"],
    {
      cwd: root,
      stdio: "inherit",
    },
  );
  assert.equal(build.status, 0, "release benchmark binary must build");
}
const directory = fs.mkdtempSync(path.join(os.tmpdir(), "fleximark-perf-"));
const run = (name, source) => {
  const file = path.join(directory, `${name}.md`);
  fs.writeFileSync(file, source);
  const started = performance.now();
  const result = spawnSync(binary, ["render", file], {
    encoding: "utf8",
    timeout: 30000,
    maxBuffer: 64 * 1024 * 1024,
  });
  assert.equal(result.status, 0, `${name} render failed: ${result.stderr}`);
  return { ms: performance.now() - started };
};
try {
  const cold = run("cold", "# Cold\n");
  const benchmark = spawnSync(binary, ["benchmark", "--json"], {
    encoding: "utf8",
    timeout: 60000,
    maxBuffer: 1024 * 1024,
  });
  assert.equal(benchmark.status, 0, `benchmark failed: ${benchmark.stderr}`);
  const report = JSON.parse(benchmark.stdout);
  assert.equal(report.schemaVersion, 1);
  assert.deepEqual(
    report.documents.map(({ lines }) => lines),
    [1000, 10000, 100000],
    "the benchmark must exercise exact line counts",
  );
  for (const document of report.documents) {
    assert.equal(document.editCount, 20);
    assert.equal(document.fullFallbacks, 0);
  }
  const metrics = {
    coldStartMs: cold.ms,
    render1kMs: report.documents[0].initialRenderMs,
    render10kMs: report.documents[1].initialRenderMs,
    render100kMs: report.documents[2].initialRenderMs,
    editBurstMs: Math.max(
      ...report.documents.map(({ editBurstMs }) => editBurstMs),
    ),
    patchBytes: Math.max(
      ...report.documents.map(({ maxPatchBytes }) => maxPatchBytes),
    ),
    peakMemoryBytes: report.peakMemoryBytes,
    specialRenderMs: report.specialRenderMs,
  };
  for (const [name, measured] of Object.entries(metrics)) {
    const allowed = contract.budgets[name] * multiplier;
    assert.ok(measured <= allowed, `${name} ${measured} exceeds ${allowed}`);
  }
  process.stdout.write(
    `${JSON.stringify({ schemaVersion: 1, multiplier, metrics })}\n`,
  );
} finally {
  fs.rmSync(directory, { recursive: true, force: true });
}
