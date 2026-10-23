import * as standaloneRuntime from "./adapter/standalone-runtime.test.mjs";

suite("Standalone File Test Suite", () => {
  suite(standaloneRuntime.suiteName, standaloneRuntime.suite);
});
