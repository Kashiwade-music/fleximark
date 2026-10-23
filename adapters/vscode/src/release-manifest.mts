import { readFile } from "node:fs/promises";

import {
  type RuntimeManifestEnvironment,
  verifyRuntimeBinary,
} from "./runtime-manifest.mjs";

export type ReleaseManifestEnvironment = RuntimeManifestEnvironment;
export function verifiedBundledDaemon(
  environment: ReleaseManifestEnvironment,
): Promise<string> {
  return verifyRuntimeBinary(environment, "fleximarkd");
}
export function verifiedBundledCli(
  environment: ReleaseManifestEnvironment,
): Promise<string> {
  return verifyRuntimeBinary(environment, "fleximark");
}
export function nodeReleaseManifestEnvironment(
  extensionRoot: string,
): ReleaseManifestEnvironment {
  return {
    extensionRoot,
    platform: process.platform,
    arch: process.arch,
    read: (file) => readFile(file),
  };
}
