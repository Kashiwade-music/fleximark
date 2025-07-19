import * as assert from "assert";
import * as fs from "fs/promises";
import * as sinon from "sinon";
import * as vscode from "vscode";

import {
  getDefaultFleximarkCss,
  isGlobalFleximarkCssExists,
  readGlobalFleximarkCss,
  readWorkspaceFleximarkCss,
} from "../../../src/commands/css/index.mjs";

suite("fleximark.css Utility Test Suite", () => {
  vscode.window.showInformationMessage("Start fleximark.css tests.");

  let sandbox: sinon.SinonSandbox;

  setup(() => {
    sandbox = sinon.createSandbox();
  });

  teardown(() => {
    sandbox.restore();
  });

  test("getDefaultFleximarkCss() should return default CSS string", () => {
    const css = getDefaultFleximarkCss();
    assert.strictEqual(typeof css, "string");
    assert.ok(css.length > 0, "Expected default CSS to be non-empty");
  });

  test("isGlobalFleximarkCssExists() should return true when file exists", async () => {
    const mockContext = createMockContext("/mock/path");
    sandbox.stub(fs, "access").resolves();

    const exists = await isGlobalFleximarkCssExists(mockContext);
    assert.strictEqual(exists, true);
  });

  test("isGlobalFleximarkCssExists() should return false when file does not exist", async () => {
    const mockContext = createMockContext("/mock/path");
    sandbox.stub(fs, "access").rejects();

    const exists = await isGlobalFleximarkCssExists(mockContext);
    assert.strictEqual(exists, false);
  });

  test("readGlobalFleximarkCss() should return file content if present", async () => {
    const mockContext = createMockContext("/mock/path");
    const expectedContent = "body { background: red; }";

    sandbox.stub(fs, "readFile").resolves(expectedContent);

    const content = await readGlobalFleximarkCss(mockContext);
    assert.strictEqual(content, expectedContent);
  });

  test("readGlobalFleximarkCss() should return default and show warning if file missing", async () => {
    const mockContext = createMockContext("/mock/path");
    sandbox.stub(fs, "readFile").rejects();
    const warnStub = sandbox.stub(vscode.window, "showWarningMessage");

    const content = await readGlobalFleximarkCss(mockContext);
    assert.strictEqual(content, getDefaultFleximarkCss());
    assert.ok(warnStub.calledOnce);
  });

  test("readWorkspaceFleximarkCss() should return workspace CSS content", async () => {
    const expectedContent = "h1 { color: blue; }";
    const mockWorkspaceFolder = { uri: vscode.Uri.file("/mock/workspace") };

    sandbox
      .stub(vscode.workspace, "workspaceFolders")
      .value([mockWorkspaceFolder]);
    sandbox.stub(fs, "readFile").resolves(expectedContent);

    const content = await readWorkspaceFleximarkCss();
    assert.strictEqual(content, expectedContent);
  });

  test("readWorkspaceFleximarkCss() should return empty string if file does not exist", async () => {
    const mockWorkspaceFolder = { uri: vscode.Uri.file("/mock/workspace") };

    sandbox
      .stub(vscode.workspace, "workspaceFolders")
      .value([mockWorkspaceFolder]);
    sandbox.stub(fs, "readFile").rejects();

    const content = await readWorkspaceFleximarkCss();
    assert.strictEqual(content, "");
  });

  test("readWorkspaceFleximarkCss() should return empty string if no workspace folder", async () => {
    sandbox.stub(vscode.workspace, "workspaceFolders").value(undefined);

    const content = await readWorkspaceFleximarkCss();
    assert.strictEqual(content, "");
  });
});

// Utility function for creating a mock ExtensionContext
function createMockContext(path: string): vscode.ExtensionContext {
  return {
    globalStorageUri: vscode.Uri.file(path),
  } as unknown as vscode.ExtensionContext;
}
