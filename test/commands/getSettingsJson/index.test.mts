/* eslint-disable @typescript-eslint/no-explicit-any */
import * as assert from "assert";
import * as fs from "fs";
import * as sinon from "sinon";
import * as vscode from "vscode";

const {
  loadJsonIfExists,
  getL10nJsonPath,
  sortObjectKeys,
  addCommentsToJson,
  objectToJsonLines,
} = (globalThis as any).commands.genSettingsJson;

export const suiteName = "genSettingsJson Utility Tests";
export const suite = () => {
  vscode.window.showInformationMessage("Start genSettingsJson utility tests.");

  let sandbox: sinon.SinonSandbox;

  setup(() => {
    sandbox = sinon.createSandbox();
  });

  teardown(() => {
    sandbox.restore();
  });

  test("loadJsonIfExists should return parsed JSON if file exists", () => {
    const dummyJson = { a: 1, b: 2 };
    sandbox.stub(fs, "existsSync").returns(true);
    sandbox.stub(fs, "readFileSync").returns(JSON.stringify(dummyJson));

    const result = loadJsonIfExists("dummy/path.json");
    assert.deepStrictEqual(result, dummyJson);
  });

  test("loadJsonIfExists should return null if file does not exist", () => {
    sandbox.stub(fs, "existsSync").returns(false);

    const result = loadJsonIfExists("nonexistent.json");
    assert.strictEqual(result, null);
  });

  test("loadJsonIfExists should return null on JSON parse error", () => {
    sandbox.stub(fs, "existsSync").returns(true);
    sandbox.stub(fs, "readFileSync").returns("invalid-json");

    const result = loadJsonIfExists("bad.json");
    assert.strictEqual(result, null);
  });

  test("getL10nJsonPath should return localized file path if it exists", () => {
    sandbox
      .stub(fs, "existsSync")
      .callsFake((p) => String(p).includes("ja.json"));
    sandbox.stub(vscode.env, "language").value("ja");

    const context = {
      extensionPath: "/ext/path",
    } as unknown as vscode.ExtensionContext;

    const result = getL10nJsonPath(context);
    assert.ok(result.endsWith("ja.json"), "Should return ja.json path");
  });

  test("getL10nJsonPath should fallback to en.json if localized file does not exist", () => {
    sandbox.stub(fs, "existsSync").returns(false);
    sandbox.stub(vscode.env, "language").value("fr");

    const context = {
      extensionPath: "/ext/path",
    } as unknown as vscode.ExtensionContext;

    const result = getL10nJsonPath(context);
    assert.ok(result.endsWith("en.json"), "Should fallback to en.json");
  });

  test("sortObjectKeys should order keys by KeyOrder then alphabetical", () => {
    const unordered = {
      z: "last",
      "fleximark.noteTemplates": "template",
      a: "first",
    };
    const result = sortObjectKeys(unordered);
    const keys = Object.keys(result);
    assert.deepStrictEqual(
      keys,
      ["fleximark.noteTemplates", "a", "z"],
      "Keys should be sorted with priority",
    );
  });

  test("addCommentsToJson should insert comments above matching keys", () => {
    const lines = ['  "key1": "value1",', '  "key2": "value2"'];
    const comments = {
      key1: ["This is key1"],
    };
    const result = addCommentsToJson(lines, comments);

    assert.ok(result.includes("  // This is key1"), "Should include comment");
    assert.ok(
      result.join("\n").includes('"key1":'),
      "Should preserve original line",
    );
  });

  test("objectToJsonLines should convert object to JSON lines", () => {
    const obj = { a: 1, b: 2 };
    const result = objectToJsonLines(obj);
    assert.ok(Array.isArray(result));
    assert.ok(result.some((line) => line.includes('"a": 1')));
  });
};
