import { createHash } from "node:crypto";
import * as path from "node:path";

import { PROTOCOL_VERSION } from "../../../contracts/typescript/protocol.mjs";

export interface RuntimeManifestEnvironment {
  extensionRoot: string;
  platform: NodeJS.Platform;
  arch: string;
  read(path: string): Promise<Buffer>;
}
const targets: Record<string, string> = {
  "win32-x64": "x86_64-pc-windows-msvc",
  "win32-arm64": "aarch64-pc-windows-msvc",
  "linux-x64": "x86_64-unknown-linux-gnu",
  "linux-arm64": "aarch64-unknown-linux-gnu",
  "darwin-x64": "x86_64-apple-darwin",
  "darwin-arm64": "aarch64-apple-darwin",
};
export function runtimeTarget(
  platform: string,
  arch: string,
): string | undefined {
  return targets[platform + "-" + arch];
}
export async function verifyRuntimeBinary(
  environment: RuntimeManifestEnvironment,
  name: "fleximark" | "fleximarkd",
): Promise<string> {
  const root = path.join(environment.extensionRoot, "runtime");
  const value: unknown = JSON.parse(
    (await environment.read(path.join(root, "manifest.json"))).toString("utf8"),
  );
  if (value === null || typeof value !== "object" || Array.isArray(value))
    throw new Error("Invalid FlexiMark runtime manifest");
  const manifest = value as Record<string, unknown>;
  if (
    manifest.schemaVersion !== 1 ||
    manifest.protocolVersion !== PROTOCOL_VERSION ||
    typeof manifest.version !== "string" ||
    manifest.target !==
      targets[environment.platform + "-" + environment.arch] ||
    !targets[environment.platform + "-" + environment.arch] ||
    manifest.files === null ||
    typeof manifest.files !== "object" ||
    Array.isArray(manifest.files)
  )
    throw new Error("Unsupported FlexiMark runtime distribution");
  const files = manifest.files as Record<string, unknown>;
  for (const [file, checksum] of Object.entries(files)) {
    if (
      !file ||
      file.includes("\\") ||
      file.startsWith("/") ||
      /^[A-Za-z]:/.test(file) ||
      file.split("/").some((p) => !p || p === "." || p === "..") ||
      typeof checksum !== "string" ||
      !/^[0-9a-f]{64}$/.test(checksum)
    )
      throw new Error("Invalid FlexiMark runtime manifest entry");
  }
  const file = "bin/" + name + (environment.platform === "win32" ? ".exe" : "");
  const checksum = files[file];
  if (typeof checksum !== "string")
    throw new Error("FlexiMark runtime binary is missing");
  const binary = path.join(root, ...file.split("/"));
  const actual = createHash("sha256")
    .update(await environment.read(binary))
    .digest("hex");
  if (actual !== checksum)
    throw new Error("Bundled FlexiMark binary checksum mismatch");
  return binary;
}
