import { afterEach, beforeEach, describe, it } from "node:test";

import * as daemonSupervisor from "./adapter/daemon-supervisor.test.mjs";
import * as releaseManifest from "./adapter/release-manifest.test.mjs";
import * as rpc from "./adapter/rpc.test.mjs";
import * as browserHost from "./browser-host.test.mjs";
import * as previewClient from "./preview-client.test.mjs";
import * as protocolContract from "./protocol-contract.test.mjs";

// The suites continue to use Mocha globals in the VS Code Electron runner. Map only the
// registration primitives they need to Node's standard test runner for this independent entry.
Object.assign(globalThis, {
  setup: beforeEach,
  teardown: afterEach,
  test: it,
});

describe(rpc.suiteName, rpc.suite);
describe(daemonSupervisor.suiteName, daemonSupervisor.suite);
describe(releaseManifest.suiteName, releaseManifest.suite);
describe(browserHost.suiteName, browserHost.suite);
describe(previewClient.suiteName, previewClient.suite);
describe(protocolContract.suiteName, protocolContract.suite);
