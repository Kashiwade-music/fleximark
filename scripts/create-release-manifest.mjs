import { createHash } from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import process from "node:process";

const targets = [
  ["linux", "x64", "fleximarkd"],
  ["darwin", "x64", "fleximarkd"],
  ["win32", "x64", "fleximarkd.exe"],
];
const artifacts = [];
for (const [platform, arch, executable] of targets) {
  const relativePath = path.posix.join(
    "bin",
    platform + "-" + arch,
    executable,
  );
  if (!fs.existsSync(relativePath)) continue;
  artifacts.push({
    platform,
    arch,
    path: relativePath,
    sha256: createHash("sha256")
      .update(fs.readFileSync(relativePath))
      .digest("hex"),
  });
}
if (
  process.argv.includes("--require-all") &&
  artifacts.length !== targets.length
)
  throw new Error("release requires Windows, macOS, and Linux x64 daemons");
fs.mkdirSync("bin", { recursive: true });
fs.writeFileSync(
  "bin/manifest.json",
  JSON.stringify({ schemaVersion: 1, protocolVersion: 1, artifacts }, null, 2) +
    "\n",
);
