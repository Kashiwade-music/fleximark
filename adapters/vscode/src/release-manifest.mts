import { constants } from "node:fs";
import { lstat, open, realpath } from "node:fs/promises";
import * as path from "node:path";

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
    read: async (file, maximumBytes) => {
      const runtime = path.join(extensionRoot, "runtime");
      if ((await lstat(runtime)).isSymbolicLink())
        throw new Error("Invalid linked runtime directory");
      const root = await realpath(runtime);
      const resolved = await realpath(file);
      const relative = path.relative(root, resolved);
      if (!relative || relative.startsWith("..") || path.isAbsolute(relative))
        throw new Error("Invalid runtime containment");
      let current = runtime;
      for (const part of path.relative(runtime, file).split(path.sep)) {
        current = path.join(current, part);
        if ((await lstat(current)).isSymbolicLink())
          throw new Error("Invalid linked runtime payload");
      }
      const handle = await open(
        resolved,
        constants.O_RDONLY | constants.O_NOFOLLOW,
      );
      try {
        const metadata = await handle.stat();
        if (
          !metadata.isFile() ||
          metadata.size === 0 ||
          metadata.size > maximumBytes
        )
          throw new Error("Invalid runtime payload size");
        const bytes = await handle.readFile();
        if (bytes.length > maximumBytes)
          throw new Error("Invalid runtime payload size");
        return bytes;
      } finally {
        await handle.close();
      }
    },
  };
}
