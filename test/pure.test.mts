import { afterEach, beforeEach, describe, it } from "node:test";

import * as rpc from "./adapter/rpc.test.mjs";
import * as previewClient from "./preview-client.test.mjs";

// The suites continue to use Mocha globals in the VS Code Electron runner. Map only the
// registration primitives they need to Node's standard test runner for this independent entry.
Object.assign(globalThis, {
  setup: beforeEach,
  teardown: afterEach,
  test: it,
});

describe(rpc.suiteName, rpc.suite);
describe(previewClient.suiteName, previewClient.suite);
