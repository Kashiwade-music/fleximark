import fs from "node:fs/promises";
import path from "node:path";
import process from "node:process";
import { fileURLToPath } from "node:url";

const workspace = path.dirname(path.dirname(fileURLToPath(import.meta.url)));
const executable =
  process.platform === "win32" ? "fleximarkd.exe" : "fleximarkd";
const source = path.join(workspace, "target", "release", executable);
const destination = path.join(
  workspace,
  "bin",
  `${process.platform}-${process.arch}`,
  executable,
);

await fs.mkdir(path.dirname(destination), { recursive: true });
await fs.copyFile(source, destination);
if (process.platform !== "win32") await fs.chmod(destination, 0o755);
