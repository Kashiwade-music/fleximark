import * as path from "node:path";
import { pathToFileURL } from "node:url";
import type * as vscode from "vscode";

import {
  ExtensionMode,
  Position,
  Range,
  Selection,
  Uri,
  __test,
} from "./vscode-pure-stub.mjs";

export function setupRuntimeVscode(root: string, initialText = "# Example\n") {
  __test.reset();
  const workspace = {
    uri: new Uri(pathToFileURL(root).href),
    name: path.basename(root),
    index: 0,
  };
  __test.workspaceFolders = [workspace];
  __test.daemonPath = process.execPath;
  let text = initialText;
  const document = {
    uri: new Uri(pathToFileURL(path.join(root, "example.md")).href),
    fileName: path.join(root, "example.md"),
    languageId: "markdown",
    version: 1,
    get lineCount() {
      return text.split("\n").length;
    },
    getText: () => text,
    lineAt: (line: number) => ({ text: text.split("\n")[line] ?? "" }),
    positionAt: (offset: number) => {
      const lines = text.slice(0, offset).split("\n");
      return new Position(lines.length - 1, lines.at(-1)?.length ?? 0);
    },
    offsetAt: (position: { line: number; character: number }) =>
      text
        .split("\n")
        .slice(0, position.line)
        .reduce((offset, line) => offset + line.length + 1, 0) +
      position.character,
    update(value: string) {
      text = value;
      this.version += 1;
    },
  };
  const reveals: unknown[] = [];
  const editor = {
    document,
    selection: new Selection(new Position(0, 0), new Position(0, 0)),
    viewColumn: 1,
    visibleRanges: [new Range(0, 0, 0, 0)],
    revealRange: (range: unknown) => {
      reveals.push(range);
    },
    reveals,
  };
  __test.textDocuments = [document];
  __test.visibleTextEditors = [editor];
  __test.activeTextEditor = editor;
  __test.showTextDocument = async () => editor;
  const context = {
    extensionUri: new Uri(pathToFileURL(root).href),
    extensionMode: ExtensionMode.Production,
    extension: { packageJSON: { version: "0.0.0" } },
    subscriptions: [],
  } as unknown as vscode.ExtensionContext;
  return {
    context,
    workspace: workspace as unknown as vscode.WorkspaceFolder,
    document: document as typeof document & vscode.TextDocument,
    editor: editor as typeof editor & vscode.TextEditor,
    get panels() {
      return __test.panels;
    },
    get externalUrls() {
      return __test.externalUrls;
    },
    get errors() {
      return __test.errors;
    },
    addWorkspace(directory: string) {
      const folder = {
        uri: new Uri(pathToFileURL(directory).href),
        name: path.basename(directory),
        index: __test.workspaceFolders?.length ?? 0,
      };
      __test.workspaceFolders?.push(folder);
      return folder as unknown as vscode.WorkspaceFolder;
    },
    removeWorkspace(uri: string) {
      __test.workspaceFolders = __test.workspaceFolders?.filter(
        (folder) => folder.uri.toString() !== uri,
      );
    },
    reset: () => __test.reset(),
  };
}
