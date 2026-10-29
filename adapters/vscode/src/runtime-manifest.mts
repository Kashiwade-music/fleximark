import { validateContractSchema } from "@fleximark/contracts/contract-validator";
import {
  DISTRIBUTION_LIMITS,
  DISTRIBUTION_TARGETS,
  RUNTIME_MANIFEST_SCHEMA,
} from "@fleximark/contracts/distribution.generated";
import { isRecord } from "@fleximark/contracts/guards";
import { PROTOCOL_VERSION } from "@fleximark/contracts/protocol";
import { createHash } from "node:crypto";
import * as path from "node:path";

export interface RuntimeManifestEnvironment {
  extensionRoot: string;
  platform: NodeJS.Platform;
  arch: string;
  read(path: string, maximumBytes: number): Promise<Buffer>;
}
export async function verifyRuntimeBinary(
  environment: RuntimeManifestEnvironment,
  name: "fleximark" | "fleximarkd",
): Promise<string> {
  const root = path.join(environment.extensionRoot, "runtime");
  const encoded = await environment.read(
    path.join(root, "manifest.json"),
    DISTRIBUTION_LIMITS.manifestBytes,
  );
  if (encoded.length > DISTRIBUTION_LIMITS.manifestBytes)
    throw new Error("Invalid FlexiMark runtime manifest size");
  const value: unknown = JSON.parse(encoded.toString("utf8"));
  if (!isRecord(value)) throw new Error("Invalid FlexiMark runtime manifest");
  const manifest = value;
  const target = DISTRIBUTION_TARGETS.find(
    (target) =>
      target.os === environment.platform && target.arch === environment.arch,
  );
  if (
    !target ||
    manifest.schemaVersion !== 1 ||
    manifest.protocolVersion !== PROTOCOL_VERSION ||
    manifest.target !== target.triple
  )
    throw new Error("Unsupported FlexiMark runtime distribution");
  if (
    !validateContractSchema(
      RUNTIME_MANIFEST_SCHEMA.$defs,
      RUNTIME_MANIFEST_SCHEMA,
      value,
    )
  )
    throw new Error(
      "Invalid FlexiMark runtime manifest: missing or unexpected payloads",
    );
  const files = manifest.files as Record<string, string>;
  const relative = `bin/${name}${target.executableSuffix}`;
  const binary = path.join(root, ...relative.split("/"));
  const bytes = await environment.read(binary, DISTRIBUTION_LIMITS.fileBytes);
  if (bytes.length === 0 || bytes.length > DISTRIBUTION_LIMITS.fileBytes)
    throw new Error("Invalid FlexiMark runtime binary size");
  if (createHash("sha256").update(bytes).digest("hex") !== files[relative])
    throw new Error("Bundled FlexiMark binary checksum mismatch");
  return binary;
}
