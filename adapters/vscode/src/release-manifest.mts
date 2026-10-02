import { createHash } from "node:crypto";
import { readFile } from "node:fs/promises";
import * as path from "node:path";

interface ReleaseArtifact {
  platform: string;
  arch: string;
  path: string;
  sha256: string;
}

interface ReleaseManifest {
  schemaVersion: number;
  protocolVersion: number;
  artifacts: ReleaseArtifact[];
}

const RELEASE_MANIFEST_KEYS = [
  "artifacts",
  "protocolVersion",
  "schemaVersion",
] as const;
const RELEASE_ARTIFACT_KEYS = ["arch", "path", "platform", "sha256"] as const;

function isRecord(value: unknown): value is Record<string, unknown> {
  return value !== null && typeof value === "object" && !Array.isArray(value);
}

function hasExactKeys(
  value: Record<string, unknown>,
  expected: readonly string[],
): boolean {
  const keys = Object.keys(value).sort();
  return (
    keys.length === expected.length &&
    keys.every((key, index) => key === expected[index])
  );
}

function unsupportedManifest(): never {
  throw new Error("Unsupported FlexiMark release manifest");
}

function isEscapingArtifactPath(artifactPath: string): boolean {
  return (
    artifactPath === ".." ||
    artifactPath.startsWith("../") ||
    path.posix.isAbsolute(artifactPath) ||
    /^[A-Za-z]:/.test(artifactPath)
  );
}

function isCanonicalArtifactPath(artifactPath: string): boolean {
  if (!artifactPath || artifactPath.includes("\\")) return false;
  const segments = artifactPath.split("/");
  return (
    !isEscapingArtifactPath(artifactPath) &&
    segments.every(
      (segment) => segment && segment !== "." && segment !== "..",
    ) &&
    path.posix.normalize(artifactPath) === artifactPath
  );
}

export interface ReleaseManifestEnvironment {
  extensionRoot: string;
  platform: NodeJS.Platform;
  arch: string;
  read(path: string): Promise<Buffer>;
}

export async function verifiedBundledDaemon(
  environment: ReleaseManifestEnvironment,
): Promise<string> {
  const value: unknown = JSON.parse(
    (
      await environment.read(
        path.join(environment.extensionRoot, "bin", "manifest.json"),
      )
    ).toString("utf8"),
  );
  if (
    !isRecord(value) ||
    !hasExactKeys(value, RELEASE_MANIFEST_KEYS) ||
    value.schemaVersion !== 1 ||
    value.protocolVersion !== 5 ||
    !Array.isArray(value.artifacts)
  )
    unsupportedManifest();
  const manifest = value as unknown as ReleaseManifest;
  for (const candidate of manifest.artifacts) {
    if (
      !isRecord(candidate) ||
      !hasExactKeys(candidate, RELEASE_ARTIFACT_KEYS) ||
      typeof candidate.platform !== "string" ||
      !candidate.platform ||
      typeof candidate.arch !== "string" ||
      !candidate.arch ||
      typeof candidate.path !== "string" ||
      typeof candidate.sha256 !== "string"
    )
      unsupportedManifest();
    if (!/^[0-9a-f]{64}$/.test(candidate.sha256))
      throw new Error("FlexiMark release manifest has an invalid checksum");
    if (isEscapingArtifactPath(candidate.path))
      throw new Error("FlexiMark release manifest path escapes the extension");
    if (!isCanonicalArtifactPath(candidate.path)) unsupportedManifest();
  }
  const matchingArtifacts = manifest.artifacts.filter(
    (item) =>
      item.platform === environment.platform && item.arch === environment.arch,
  );
  if (!matchingArtifacts.length)
    throw new Error(
      `FlexiMark does not support ${environment.platform}-${environment.arch}`,
    );
  if (matchingArtifacts.length !== 1) unsupportedManifest();
  const [artifact] = matchingArtifacts;
  const binary = path.resolve(
    environment.extensionRoot,
    ...artifact.path.split("/"),
  );
  const relativeBinary = path.relative(environment.extensionRoot, binary);
  if (
    relativeBinary === ".." ||
    relativeBinary.startsWith(`..${path.sep}`) ||
    path.isAbsolute(relativeBinary)
  )
    throw new Error("FlexiMark release manifest path escapes the extension");
  const checksum = createHash("sha256")
    .update(await environment.read(binary))
    .digest("hex");
  if (checksum !== artifact.sha256)
    throw new Error(
      "Bundled FlexiMark daemon is corrupt or does not match this extension",
    );
  return binary;
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
