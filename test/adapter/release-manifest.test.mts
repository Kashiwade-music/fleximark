import * as assert from "node:assert/strict";
import { createHash } from "node:crypto";
import * as path from "node:path";

import {
  type ReleaseManifestEnvironment,
  verifiedBundledDaemon,
} from "../../adapters/vscode/src/release-manifest.mjs";

export const suiteName = "Release manifest verifier";

const binaryBytes = Buffer.from("daemon-binary");
const binaryHash = createHash("sha256").update(binaryBytes).digest("hex");

function environment(manifest: unknown): ReleaseManifestEnvironment {
  const extensionRoot = path.resolve("release-manifest-fixture");
  const binary = path.join(extensionRoot, "bin", "fleximarkd");
  return {
    extensionRoot,
    platform: "win32",
    arch: "x64",
    read: async (file) => {
      if (file === path.join(extensionRoot, "bin", "manifest.json"))
        return Buffer.from(JSON.stringify(manifest));
      if (file === binary) return binaryBytes;
      throw new Error(`unexpected read: ${file}`);
    },
  };
}

function artifact(overrides: Record<string, unknown> = {}): object {
  return {
    platform: "win32",
    arch: "x64",
    path: "bin/fleximarkd",
    sha256: binaryHash,
    ...overrides,
  };
}

function manifest(artifacts: unknown[], overrides = {}): object {
  return {
    schemaVersion: 1,
    protocolVersion: 5,
    artifacts,
    ...overrides,
  };
}

export function suite(): void {
  test("accepts one exact current-target artifact", async () => {
    const fixture = environment(manifest([artifact()]));
    assert.equal(
      await verifiedBundledDaemon(fixture),
      path.join(fixture.extensionRoot, "bin", "fleximarkd"),
    );
  });

  test("rejects malformed root and artifact shapes with the stable error", async () => {
    const cases: unknown[] = [
      null,
      { schemaVersion: 1, protocolVersion: 5, artifacts: [], unknown: true },
      { schemaVersion: 1, artifacts: [] },
      manifest([null]),
      manifest([artifact({ arch: 1 })]),
      manifest([artifact({ unknown: "field" })]),
      manifest([artifact({ path: "bin\\fleximarkd" })]),
      manifest([artifact({ path: "bin/./fleximarkd" })]),
      manifest([artifact({ path: "bin//fleximarkd" })]),
      manifest([artifact(), artifact()]),
    ];
    for (const value of cases)
      await assert.rejects(verifiedBundledDaemon(environment(value)), {
        message: "Unsupported FlexiMark release manifest",
      });
  });

  test("distinguishes unsupported targets, escaping paths, and checksum errors", async () => {
    await assert.rejects(
      verifiedBundledDaemon(
        environment(manifest([artifact({ platform: "linux" })])),
      ),
      { message: "FlexiMark does not support win32-x64" },
    );
    for (const artifactPath of [
      "../fleximarkd",
      "/bin/fleximarkd",
      "C:/bin/fleximarkd",
    ])
      await assert.rejects(
        verifiedBundledDaemon(
          environment(manifest([artifact({ path: artifactPath })])),
        ),
        { message: "FlexiMark release manifest path escapes the extension" },
      );
    await assert.rejects(
      verifiedBundledDaemon(
        environment(manifest([artifact({ sha256: binaryHash.toUpperCase() })])),
      ),
      { message: "FlexiMark release manifest has an invalid checksum" },
    );
  });
}
