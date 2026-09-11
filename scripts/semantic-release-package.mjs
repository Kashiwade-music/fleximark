import { execFileSync } from "node:child_process";
import process from "node:process";

export async function prepare() {
  const npm = process.platform === "win32" ? "npm.cmd" : "npm";
  execFileSync(
    npm,
    ["exec", "--", "vsce", "package", "--out", "fleximark.vsix"],
    { stdio: "inherit" },
  );
}
