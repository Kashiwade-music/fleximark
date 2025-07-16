/* eslint-disable @typescript-eslint/no-explicit-any */
import * as fs from "fs";
import * as path from "path";
import * as vscode from "vscode";

const KeyOrder = [
  "fleximark.noteCategories",
  "fleximark.noteFileNamePrefix",
  "fleximark.noteFileNameSuffix",
  "fleximark.noteTemplates",
  "markdown.copyFiles.destination",
  "fleximark.defaultPreviewMode",
];

const genSettingsJson = async (
  context: vscode.ExtensionContext,
  workspacePath: string,
): Promise<string> => {
  const vscodeWorkspaceSettingsJsonPath = path.join(
    workspacePath,
    ".vscode",
    "settings.json",
  );
  const baseJsonPath = path.join(
    context.extensionPath,
    "media",
    "workspaceSettingsJsonTemplate",
    "base.json",
  );
  const commentl10nJsonPath = getL10nJsonPath(context);

  const baseJson = loadJsonIfExists(baseJsonPath) || {};
  const workspaceJson = loadJsonIfExists(vscodeWorkspaceSettingsJsonPath) || {};
  const commentl10nJson = (loadJsonIfExists(commentl10nJsonPath) ||
    {}) as Record<string, string[]>;

  const mergedJson = {
    ...baseJson,
    ...workspaceJson,
  };

  const sortedJson = sortObjectKeys(mergedJson);

  const sortedJsonWithComments = addCommentsToJson(
    objectToJsonLines(sortedJson),
    commentl10nJson,
  );

  return sortedJsonWithComments.join("\n") + "\n";
};

export default genSettingsJson;

function parseJsonc(jsonString: string): any {
  let result = "";
  let inString = false;
  let inSingleLineComment = false;
  let inMultiLineComment = false;
  let prevChar = "";

  for (let i = 0; i < jsonString.length; i++) {
    const char = jsonString[i];
    const nextChar = jsonString[i + 1];

    if (inSingleLineComment) {
      if (char === "\n") {
        inSingleLineComment = false;
        result += char;
      }
      continue;
    }

    if (inMultiLineComment) {
      if (char === "*" && nextChar === "/") {
        inMultiLineComment = false;
        i++; // skip '/'
      }
      continue;
    }

    if (!inString && char === "/" && nextChar === "/") {
      inSingleLineComment = true;
      i++; // skip next '/'
      continue;
    }

    if (!inString && char === "/" && nextChar === "*") {
      inMultiLineComment = true;
      i++; // skip next '*'
      continue;
    }

    if (char === '"' && prevChar !== "\\") {
      inString = !inString;
    }

    result += char;
    prevChar = char;
  }

  try {
    return JSON.parse(result);
  } catch (error) {
    console.error("Failed to parse JSONC:", error);
    return {};
  }
}

function loadJsonIfExists(filePath: string): Record<string, unknown> | null {
  if (!fs.existsSync(filePath)) {
    return null;
  }

  try {
    const data = fs.readFileSync(filePath, "utf-8");
    return parseJsonc(data) as Record<string, unknown>;
  } catch {
    return null;
  }
}

function getL10nJsonPath(context: vscode.ExtensionContext) {
  const baseDir = path.join(
    context.extensionPath,
    "media",
    "workspaceSettingsJsonTemplate",
    "l10n",
  );
  const targetPath = path.join(baseDir, `${vscode.env.language}.json`);
  if (fs.existsSync(targetPath)) {
    return targetPath;
  }
  return path.join(baseDir, "en.json");
}

const sortObjectKeys = (obj: Record<string, any>): object => {
  const sortedObj: Record<string, any> = {};

  for (const key of KeyOrder) {
    if (key in obj) {
      sortedObj[key] = obj[key];
    }
  }

  for (const key of Object.keys(obj).sort()) {
    if (!(key in sortedObj)) {
      sortedObj[key] = obj[key];
    }
  }

  return sortedObj;
};

function addCommentsToJson(
  jsonLines: string[],
  comments: Record<string, string[]>,
): string[] {
  const result: string[] = [];
  let isFirstMatch = true;

  for (const line of jsonLines) {
    const match = line.match(/^(\s*)"([^"]+)":/);
    if (match) {
      // eslint-disable-next-line @typescript-eslint/no-unused-vars
      const [_, indent, key] = match;
      comments[key]?.forEach((commentLine, index) => {
        if (index === 0 && !isFirstMatch) {
          // Add a blank line before the first comment
          result.push("");
          isFirstMatch = false;
        }
        result.push(`${indent}// ${commentLine}`);
      });
    }
    result.push(line);
  }

  return result;
}

function objectToJsonLines(obj: object): string[] {
  const jsonString = JSON.stringify(obj, null, 2);
  return jsonString.split("\n");
}
