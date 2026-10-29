import { AdapterLog } from "fleximark/src/adapter-log";
import * as assert from "node:assert/strict";

import { __test } from "../support/vscode-pure-stub.mjs";

export const suiteName = "Adapter diagnostic levels";

export function suite(): void {
  test("filters error, info and debug and redacts accepted diagnostics", () => {
    for (const [level, expected] of [
      ["off", []],
      ["error", ["error token=<redacted>"]],
      ["info", ["error token=<redacted>", "info"]],
      ["debug", ["error token=<redacted>", "info", "debug"]],
    ] as const) {
      __test.reset();
      __test.logLevel = level;
      const logger = new AdapterLog();
      logger.stderr("error token=credential");
      logger.stderr("info");
      logger.stderr("debug");
      assert.deepEqual(__test.outputLines, expected, level);
      logger.dispose();
    }
    __test.reset();
  });
}
