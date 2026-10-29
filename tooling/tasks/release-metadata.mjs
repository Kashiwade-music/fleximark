import { readFileSync } from "node:fs";
import { URL } from "node:url";

export function releaseMetadata() {
  const fields = new Map();
  for (const raw of readFileSync(
    new URL("../../release.toml", import.meta.url),
    "utf8",
  ).split(/\r?\n/)) {
    const line = raw.trim();
    if (!line || line.startsWith("#")) continue;
    const separator = line.indexOf("=");
    const name = line.slice(0, separator).trim();
    if (
      separator < 0 ||
      !["version", "protocol_version"].includes(name) ||
      fields.has(name)
    )
      throw new Error("Invalid release metadata field");
    fields.set(name, line.slice(separator + 1).trim());
  }
  const version =
    /^"((0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(?:-[0-9A-Za-z.-]+)?(?:\+[0-9A-Za-z.-]+)?)"$/.exec(
      fields.get("version") ?? "",
    )?.[1];
  const encodedProtocol = fields.get("protocol_version") ?? "";
  const protocolVersion = Number(encodedProtocol);
  if (
    !version ||
    !/^[1-9]\d*$/.test(encodedProtocol) ||
    !Number.isSafeInteger(protocolVersion) ||
    protocolVersion > 0xffffffff
  )
    throw new Error("Invalid release version");
  return { version, protocolVersion };
}
