import * as assert from "assert";
import * as vscode from "vscode";

import { getDefaultFleximarkCss } from "../../../src/commands/css/index.mjs";

export const suiteName = "fleximark.css Utility Tests";
export const suite = () => {
  console.log("Start fleximark.css tests.");

  test("getDefaultFleximarkCss() should return default CSS string", () => {
    const css = getDefaultFleximarkCss();
    assert.strictEqual(typeof css, "string");
    assert.ok(css.length > 0, "Expected default CSS to be non-empty");

    const expectedBody = `/*
=========================
Reset 
=========================
*/`;
    assert.ok(
      css.includes(expectedBody),
      "Check if default CSS contains expected body",
    );
  });
};
