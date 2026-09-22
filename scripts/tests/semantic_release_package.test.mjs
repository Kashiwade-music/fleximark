import assert from "node:assert/strict";
import test from "node:test";

import { createPrepare } from "../semantic-release-package.mjs";

const release = {
  nextRelease: {
    version: "1.2.3",
    gitTag: "v1.2.3",
  },
};

const sourceGitHead = "fedcba9876543210fedcba9876543210fedcba98";

function harness({ platform = "linux", exists = () => false, failureAt } = {}) {
  const calls = [];
  const run = (...args) => {
    calls.push(args);
    if (calls.length === failureAt) throw new Error(`failure ${failureAt}`);
  };
  return {
    calls,
    prepare: createPrepare({
      run,
      exists,
      platform,
      environment: {
        PRESERVED: "yes",
        FLEXIMARK_RELEASE_SOURCE_GIT_HEAD: sourceGitHead,
        GITHUB_TOKEN: "github-secret",
        GH_TOKEN: "gh-secret",
        NPM_TOKEN: "npm-secret",
        NODE_AUTH_TOKEN: "node-secret",
        VSCE_PAT: "marketplace-secret",
        CUSTOM_API_KEY: "api-secret",
      },
    }),
  };
}

test("packages exactly once before creating the bound identity", async () => {
  const { calls, prepare } = harness();
  await prepare({}, release);

  assert.equal(calls.length, 2);
  assert.deepEqual(calls[0][0], "yarn");
  assert.deepEqual(calls[0][1], [
    "exec",
    "vsce",
    "package",
    "--no-dependencies",
    "--out",
    "fleximark.vsix",
  ]);
  assert.equal(calls[0][2].env.FLEXIMARK_RELEASE_PREBUILT, "1");
  assert.equal(calls[0][2].env.PRESERVED, "yes");
  assert.deepEqual(calls[1][0], "python");
  assert.deepEqual(calls[1][1].slice(-6), [
    "--expected-version",
    "1.2.3",
    "--git-tag",
    "v1.2.3",
    "--source-git-head",
    sourceGitHead,
  ]);
  for (const call of calls) {
    assert.equal(call[2].env.PRESERVED, "yes");
    for (const credential of [
      "GITHUB_TOKEN",
      "GH_TOKEN",
      "NPM_TOKEN",
      "NODE_AUTH_TOKEN",
      "VSCE_PAT",
      "CUSTOM_API_KEY",
    ]) {
      assert.equal(call[2].env[credential], undefined);
    }
  }
});

test("rejects stale outputs without invoking a process", async () => {
  const { calls, prepare } = harness({ exists: () => true });
  await assert.rejects(prepare({}, release), /stale release artifact/);
  assert.equal(calls.length, 0);
});

test("propagates package and identity creation failures", async () => {
  for (const failureAt of [1, 2]) {
    const { calls, prepare } = harness({ failureAt });
    await assert.rejects(
      prepare({}, release),
      new RegExp(`failure ${failureAt}`),
    );
    assert.equal(calls.length, failureAt, `failure at call ${failureAt}`);
  }
});

test("uses Windows command names without changing arguments", async () => {
  const { calls, prepare } = harness({ platform: "win32" });
  await prepare({}, release);
  assert.equal(calls[0][0], "yarn.cmd");
  assert.equal(calls[1][0], "python.exe");
});
