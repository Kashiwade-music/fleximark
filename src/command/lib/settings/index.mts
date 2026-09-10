/* eslint-disable @typescript-eslint/no-explicit-any */
import * as fs from "fs";
import * as path from "path";
import * as vscode from "vscode";

import * as fLibJsonc from "../jsonc/index.mjs";

export const KeyOrder = [
  "fleximark.noteCategories",
  "fleximark.noteFileNamePrefix",
  "fleximark.noteFileNameSuffix",
  "fleximark.noteTemplates",
  "markdown.copyFiles.destination",
  "fleximark.defaultPreviewMode",
  "[markdown]",
];

export async function genSettingsJson(
  context: vscode.ExtensionContext,
  workspacePath: string,
): Promise<string> {
  const vscodeWorkspaceSettingsJsonPath = path.join(
    workspacePath,
    ".vscode",
    "settings.json",
  );
  const baseJsonPath = path.join(
    context.extensionPath,
    "dist",
    "media",
    "workspaceSettingsJsonTemplate",
    "base.json",
  );
  const commentl10nJsonPath = getL10nJsonPath(context);

  const baseJson = loadJsonIfExists(baseJsonPath);
  if (!baseJson) {
    throw new Error(
      `Base JSON file not found at ${baseJsonPath}. Please ensure the file exists.`,
    );
  }
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
}

function loadJsonIfExists(filePath: string): Record<string, unknown> | null {
  if (!fs.existsSync(filePath)) {
    return null;
  }

  try {
    const data = fs.readFileSync(filePath, "utf-8");
    return fLibJsonc.parse(data) as Record<string, unknown>;
  } catch {
    return null;
  }
}

function getL10nJsonPath(context: vscode.ExtensionContext) {
  const baseDir = path.join(
    context.extensionPath,
    "dist",
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

export const sortObjectKeys = (obj: Record<string, any>): object => {
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

export function addCommentsToJson(
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
        }
        isFirstMatch = false;
        result.push(`${indent}// ${commentLine}`);
      });
    }
    result.push(line);
  }

  return result;
}

export function objectToJsonLines(obj: object): string[] {
  const jsonString = JSON.stringify(obj, null, 2);
  return jsonString.split("\n");
}
