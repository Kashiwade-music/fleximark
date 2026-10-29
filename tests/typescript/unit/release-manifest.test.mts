import {
  DISTRIBUTION_FILES,
  DISTRIBUTION_TARGETS,
} from "@fleximark/contracts/distribution.generated";
import { PROTOCOL_VERSION } from "@fleximark/contracts/protocol";
import {
  type ReleaseManifestEnvironment,
  nodeReleaseManifestEnvironment,
  verifiedBundledCli,
  verifiedBundledDaemon,
} from "fleximark/src/release-manifest";
import * as assert from "node:assert/strict";
import { createHash } from "node:crypto";
import {
  mkdir,
  mkdtemp,
  rename,
  rm,
  symlink,
  writeFile,
} from "node:fs/promises";
import { tmpdir } from "node:os";
import * as path from "node:path";

export const suiteName = "Runtime distribution verifier";
const bytes = Buffer.from("runtime-binary");
const sha256 = createHash("sha256").update(bytes).digest("hex");
function manifest(): Record<string, unknown> {
  const target = DISTRIBUTION_TARGETS.find(
    (target) => target.vscode === "win32-x64",
  );
  assert.ok(target);
  return {
    schemaVersion: 1,
    protocolVersion: PROTOCOL_VERSION,
    version: "1.0.0",
    target: "x86_64-pc-windows-msvc",
    files: Object.fromEntries(
      DISTRIBUTION_FILES.map((file) => [
        file.path.replace("{exe}", target.executableSuffix),
        sha256,
      ]),
    ),
  };
}
function environment(
  value: unknown,
  content = bytes,
): ReleaseManifestEnvironment {
  const extensionRoot = path.resolve("runtime-test");
  return {
    extensionRoot,
    platform: "win32",
    arch: "x64",
    read: async (file) => {
      if (file === path.join(extensionRoot, "runtime", "manifest.json"))
        return Buffer.from(JSON.stringify(value));
      if (
        file === path.join(extensionRoot, "runtime", "bin", "fleximarkd.exe") ||
        file === path.join(extensionRoot, "runtime", "bin", "fleximark.exe")
      )
        return content;
      throw new Error("Unexpected read " + file);
    },
  };
}
export function suite(): void {
  test("filesystem verification rejects linked binary directories", async () => {
    const root = await mkdtemp(
      path.join(tmpdir(), "fleximark-runtime-policy-"),
    );
    try {
      const target = DISTRIBUTION_TARGETS.find(
        (target) =>
          target.os === process.platform && target.arch === process.arch,
      );
      assert.ok(target);
      const files = Object.fromEntries(
        DISTRIBUTION_FILES.map((file) => [
          file.path.replace("{exe}", target.executableSuffix),
          sha256,
        ]),
      );
      for (const relative of Object.keys(files)) {
        const file = path.join(root, "runtime", relative);
        await mkdir(path.dirname(file), { recursive: true });
        await writeFile(file, bytes);
      }
      await writeFile(
        path.join(root, "runtime", "manifest.json"),
        JSON.stringify({ ...manifest(), target: target.triple, files }),
      );
      const env = nodeReleaseManifestEnvironment(root);
      assert.equal(
        await verifiedBundledDaemon(env),
        path.join(root, "runtime", `bin/fleximarkd${target.executableSuffix}`),
      );
      await rename(
        path.join(root, "runtime", "bin"),
        path.join(root, "outside-bin"),
      );
      await symlink(
        path.join(root, "outside-bin"),
        path.join(root, "runtime", "bin"),
        process.platform === "win32" ? "junction" : "dir",
      );
      await assert.rejects(
        verifiedBundledDaemon(env),
        /Invalid.*(containment|linked)/,
      );
    } finally {
      await rm(root, { recursive: true, force: true });
    }
  });
  test("verifies both server and CLI binaries from one distribution", async () => {
    const env = environment(manifest());
    assert.equal(
      await verifiedBundledDaemon(env),
      path.join(env.extensionRoot, "runtime", "bin", "fleximarkd.exe"),
    );
    assert.equal(
      await verifiedBundledCli(env),
      path.join(env.extensionRoot, "runtime", "bin", "fleximark.exe"),
    );
  });
  test("rejects incompatible targets and old artifact manifests", async () => {
    for (const value of [
      null,
      {},
      { ...manifest(), target: "aarch64-apple-darwin" },
      { ...manifest(), protocolVersion: PROTOCOL_VERSION - 1 },
      { schemaVersion: 1, protocolVersion: 5, artifacts: [] },
    ])
      await assert.rejects(
        verifiedBundledDaemon(environment(value)),
        /Invalid|Unsupported/,
      );
  });
  test("rejects unsafe paths and malformed checksums before reading binaries", async () => {
    for (const file of [
      "../escape",
      "/absolute",
      "bin//server",
      "bin/./server",
      "C:/server",
    ]) {
      await assert.rejects(
        verifiedBundledDaemon(
          environment({ ...manifest(), files: { [file]: sha256 } }),
        ),
        /Invalid/,
      );
    }
    await assert.rejects(
      verifiedBundledDaemon(
        environment({
          ...manifest(),
          files: { "bin/fleximarkd.exe": "broken" },
        }),
      ),
      /Invalid/,
    );
  });
  test("rejects missing binaries and corrupt distributions", async () => {
    await assert.rejects(
      verifiedBundledCli(environment({ ...manifest(), files: {} })),
      /missing/,
    );
    await assert.rejects(
      verifiedBundledDaemon(environment(manifest(), Buffer.from("corrupt"))),
      /checksum/,
    );
  });
}
