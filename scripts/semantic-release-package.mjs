import { execFileSync } from "node:child_process";
import { existsSync } from "node:fs";
import process from "node:process";

const credentialName =
  /(?:^|_)(?:TOKEN|SECRET|PASSWORD|PASSCODE|API_KEY|PRIVATE_KEY|CREDENTIAL|PAT|AUTH)$/i;
const sourceGitHeadPattern = /^(?:[0-9a-f]{40}|[0-9a-f]{64})$/;

function releaseChildEnvironment(environment) {
  return Object.fromEntries(
    Object.entries(environment).filter(([name]) => !credentialName.test(name)),
  );
}

export function createPrepare({
  run = execFileSync,
  exists = existsSync,
  platform = process.platform,
  environment = process.env,
} = {}) {
  return async function prepare(_pluginConfig, { nextRelease }) {
    const sourceGitHead = environment.FLEXIMARK_RELEASE_SOURCE_GIT_HEAD;
    if (
      typeof nextRelease?.version !== "string" ||
      typeof nextRelease?.gitTag !== "string" ||
      typeof sourceGitHead !== "string" ||
      !sourceGitHeadPattern.test(sourceGitHead)
    )
      throw new Error(
        "semantic-release did not provide a complete release identity",
      );

    for (const path of ["fleximark.vsix", "fleximark.vsix.identity.json"]) {
      if (exists(path))
        throw new Error(`stale release artifact exists: ${path}`);
    }
    const yarn = platform === "win32" ? "yarn.cmd" : "yarn";
    const python = platform === "win32" ? "python.exe" : "python";
    const childEnvironment = releaseChildEnvironment(environment);
    run(
      yarn,
      [
        "exec",
        "vsce",
        "package",
        "--no-dependencies",
        "--out",
        "fleximark.vsix",
      ],
      {
        stdio: "inherit",
        env: { ...childEnvironment, FLEXIMARK_RELEASE_PREBUILT: "1" },
      },
    );
    run(
      python,
      [
        "scripts/release_artifact.py",
        "create",
        "--vsix",
        "fleximark.vsix",
        "--identity",
        "fleximark.vsix.identity.json",
        "--expected-version",
        nextRelease.version,
        "--git-tag",
        nextRelease.gitTag,
        "--source-git-head",
        sourceGitHead,
      ],
      { stdio: "inherit", env: childEnvironment },
    );
  };
}

export const prepare = createPrepare();
