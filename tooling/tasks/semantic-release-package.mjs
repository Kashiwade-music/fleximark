import { execFileSync } from "node:child_process";
import { readFileSync, writeFileSync } from "node:fs";
import process from "node:process";

const credentialName =
  /(?:^|_)(?:TOKEN|SECRET|PASSWORD|PASSCODE|API_KEY|PRIVATE_KEY|CREDENTIAL|PAT|AUTH)$/i;

export function createPrepare({
  run = execFileSync,
  read = readFileSync,
  write = writeFileSync,
  platform = process.platform,
  environment = process.env,
} = {}) {
  return async function prepare(_pluginConfig, { nextRelease }) {
    if (!/^\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?$/.test(nextRelease?.version ?? ""))
      throw new Error("semantic-release did not provide a valid version");
    const metadata = read("release.toml", "utf8");
    if (!/^version = "[^"]+"$/m.test(metadata))
      throw new Error("release.toml has no canonical version");
    write(
      "release.toml",
      metadata.replace(
        /^version = "[^"]+"$/m,
        `version = "${nextRelease.version}"`,
      ),
      "utf8",
    );
    const childEnvironment = Object.fromEntries(
      Object.entries(environment).filter(
        ([name]) => !credentialName.test(name),
      ),
    );
    run(
      platform === "win32" ? "python.exe" : "python",
      ["tooling/tasks/distribution.py", "versions"],
      { stdio: "inherit", env: childEnvironment },
    );
  };
}

export const prepare = createPrepare();
