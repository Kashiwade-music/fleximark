import { execFileSync } from "node:child_process";
import process from "node:process";

export async function prepare() {
  const yarn = process.platform === "win32" ? "yarn.cmd" : "yarn";
  execFileSync(
    yarn,
    ["exec", "vsce", "package", "--no-dependencies", "--out", "fleximark.vsix"],
    { stdio: "inherit" },
  );
}
