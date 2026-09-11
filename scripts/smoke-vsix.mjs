import { downloadAndUnzipVSCode } from "@vscode/test-electron";
import { Buffer } from "node:buffer";
import { spawnSync } from "node:child_process";
import { spawn } from "node:child_process";
import { createHash } from "node:crypto";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import process from "node:process";
import { clearTimeout, setTimeout } from "node:timers";

const vsix = path.resolve(process.argv[2] ?? "fleximark.vsix");
const executable = await downloadAndUnzipVSCode("stable");
const scratch = fs.mkdtempSync(path.join(os.tmpdir(), "fleximark-install-"));
try {
  const common = [
    "--extensions-dir",
    path.join(scratch, "extensions"),
    "--user-data-dir",
    path.join(scratch, "user-data"),
  ];
  const install = spawnSync(
    executable,
    [...common, "--install-extension", vsix, "--force"],
    { encoding: "utf8" },
  );
  if (install.status !== 0)
    throw new Error(install.stderr || install.stdout || "VSIX install failed");
  const installed = spawnSync(executable, [...common, "--list-extensions"], {
    encoding: "utf8",
  });
  if (
    installed.status !== 0 ||
    !installed.stdout.toLowerCase().includes("kashiwade.fleximark")
  )
    throw new Error(
      installed.stderr || installed.stdout || "installed extension is missing",
    );

  const extensionsDir = path.join(scratch, "extensions");
  const installedDirectory = fs
    .readdirSync(extensionsDir, { withFileTypes: true })
    .find(
      (entry) =>
        entry.isDirectory() &&
        entry.name.toLowerCase().startsWith("kashiwade.fleximark-"),
    );
  if (!installedDirectory)
    throw new Error("installed extension files are missing");
  const extensionRoot = path.join(extensionsDir, installedDirectory.name);
  const manifest = JSON.parse(
    fs.readFileSync(path.join(extensionRoot, "bin", "manifest.json"), "utf8"),
  );
  if (manifest.schemaVersion !== 1)
    throw new Error("unsupported release manifest");
  const artifact = manifest.artifacts.find(
    (item) => item.platform === process.platform && item.arch === process.arch,
  );
  if (!artifact) throw new Error("manifest has no daemon for this platform");
  const daemon = path.resolve(extensionRoot, ...artifact.path.split("/"));
  const relativeDaemon = path.relative(extensionRoot, daemon);
  if (relativeDaemon.startsWith("..") || path.isAbsolute(relativeDaemon))
    throw new Error("manifest daemon path escapes the extension");
  const checksum = createHash("sha256")
    .update(fs.readFileSync(daemon))
    .digest("hex");
  if (checksum !== artifact.sha256)
    throw new Error("packaged daemon checksum does not match the manifest");

  const child = spawn(daemon, ["rpc"], {
    stdio: ["pipe", "pipe", "pipe"],
    windowsHide: true,
  });
  const response = await new Promise((resolve, reject) => {
    let buffer = Buffer.alloc(0);
    let stderr = "";
    const timer = setTimeout(() => {
      child.kill();
      reject(new Error(stderr || "packaged daemon initialize timed out"));
    }, 10_000);
    child.stderr.on("data", (chunk) => (stderr += chunk.toString()));
    child.once("error", (error) => {
      clearTimeout(timer);
      reject(error);
    });
    child.stdout.on("data", (chunk) => {
      buffer = Buffer.concat([buffer, chunk]);
      const headerEnd = buffer.indexOf("\r\n\r\n");
      if (headerEnd < 0) return;
      const length = Number(
        /Content-Length:\s*(\d+)/i.exec(
          buffer.subarray(0, headerEnd).toString("ascii"),
        )?.[1],
      );
      if (
        !Number.isSafeInteger(length) ||
        buffer.length < headerEnd + 4 + length
      )
        return;
      clearTimeout(timer);
      resolve(
        JSON.parse(
          buffer
            .subarray(headerEnd + 4, headerEnd + 4 + length)
            .toString("utf8"),
        ),
      );
    });
    const request = Buffer.from(
      JSON.stringify({
        jsonrpc: "2.0",
        id: 1,
        method: "fleximark/initialize",
        params: {
          protocolVersion: manifest.protocolVersion,
          client: { name: "release-smoke", version: "1" },
          capabilities: {},
        },
      }),
    );
    child.stdin.write("Content-Length: " + request.length + "\r\n\r\n");
    child.stdin.write(request);
  });
  child.stdin.end();
  child.kill();
  if (
    response?.result?.protocolVersion !== manifest.protocolVersion ||
    typeof response?.result?.daemonInstanceId !== "string"
  )
    throw new Error("packaged daemon protocol initialization failed");
} finally {
  fs.rmSync(scratch, { recursive: true, force: true });
}
