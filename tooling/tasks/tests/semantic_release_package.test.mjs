import assert from "node:assert/strict";
import test from "node:test";

import { createPrepare } from "../semantic-release-package.mjs";

test("release metadata is the sole version source and product generation strips credentials", async () => {
  const writes = [];
  const runs = [];
  const prepare = createPrepare({
    read: () => 'version = "1.2.3"\nprotocol_version = 5\n',
    write: (...args) => writes.push(args),
    run: (...args) => runs.push(args),
    environment: { GITHUB_TOKEN: "secret", VSCE_PAT: "secret", PATH: "tools" },
  });
  await prepare({}, { nextRelease: { version: "1.3.0" } });
  assert.equal(writes.length, 1);
  assert.equal(writes[0][0], "release.toml");
  assert.match(writes[0][1], /version = "1.3.0"/);
  assert.match(writes[0][1], /protocol_version = 5/);
  assert.deepEqual(runs[0][1], ["tooling/tasks/distribution.py", "versions"]);
  assert.deepEqual(runs[0][2].env, { PATH: "tools" });
});

test("invalid release versions cannot mutate metadata", async () => {
  let wrote = false;
  const prepare = createPrepare({
    write: () => {
      wrote = true;
    },
  });
  await assert.rejects(
    prepare({}, { nextRelease: { version: "../bad" } }),
    /valid version/,
  );
  assert.equal(wrote, false);
});

test("generator failures propagate and Windows uses Python executable", async () => {
  const prepare = createPrepare({
    read: () => 'version = "1.2.3"',
    write: () => {
      // The generator owns the failure exercised by this test.
    },
    platform: "win32",
    run: (command) => {
      assert.equal(command, "python.exe");
      throw new Error("generation failed");
    },
  });
  await assert.rejects(
    prepare({}, { nextRelease: { version: "1.2.4" } }),
    /generation failed/,
  );
});
