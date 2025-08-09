import * as fs from "fs/promises";
import path from "path";
import * as vscode from "vscode";

import * as fLibFs from "../fs/index.mjs";
import fleximarkcss from "./fleximark.css";

/**
 * Returns the default content for the fleximark.css file.
 * This CSS can be used as a base styling.
 */
export function getDefaultFleximarkCss(): string {
  return fleximarkcss;
}

/**
 * Checks whether the global fleximark.css file exists in the extension's global storage.
 *
 * @param context - The extension context containing the globalStorageUri.
 * @returns A promise that resolves to true if the file exists, or false otherwise.
 */
export async function isGlobalFleximarkCssExists(
  context: vscode.ExtensionContext,
  isMessageEmittable = true,
): Promise<boolean> {
  const cssPath = await getCssPath({
    scope: "global",
    context,
    isMessageEmittable,
  });
  if (!cssPath) {
    return false;
  }

  try {
    await fs.access(cssPath);
    return true;
  } catch {
    return false;
  }
}

/**
 * Reads the fleximark.css file from the extension's global storage.
 * If the file does not exist, returns the default CSS content and attempts to create it.
 *
 * @param context - The extension context containing the globalStorageUri.
 * @returns The content of the fleximark.css file as a string.
 */
export async function readGlobalFleximarkCss(
  context: vscode.ExtensionContext,
  isMessageEmittable = true,
): Promise<string> {
  const cssPath = await getCssPath({
    scope: "global",
    context,
    isMessageEmittable,
  });
  if (!cssPath) {
    return getDefaultFleximarkCss();
  }

  try {
    const content = await fs.readFile(cssPath, "utf8");
    return content;
  } catch {
    if (isMessageEmittable) {
      vscode.window.showWarningMessage(
        vscode.l10n.t(
          "fleximark.css not found in global storage, returning default content.",
        ),
      );
    }

    return getDefaultFleximarkCss();
  }
}

/**
 * Reads the fleximark.css file from the current workspace's .fleximark directory.
 * If the file does not exist or cannot be read, returns an empty string.
 *
 * @returns The content of the workspace fleximark.css file or an empty string.
 */
export async function readWorkspaceFleximarkCss(
  isMessageEmittable = true,
): Promise<string> {
  const cssPath = await getCssPath({
    scope: "workspace",
    isMessageEmittable,
  });
  if (!cssPath) return "";

  try {
    const content = await fs.readFile(cssPath, "utf8");
    return content;
  } catch {
    return "";
  }
}

interface GetCssPathArgsBase {
  isMessageEmittable?: boolean;
}

interface GetCssPathArgsWorkspace extends GetCssPathArgsBase {
  scope: "workspace";
}

interface GetCssPathArgsGlobal extends GetCssPathArgsBase {
  scope: "global";
  context: vscode.ExtensionContext;
}

export async function getCssPath(
  args: GetCssPathArgsWorkspace | GetCssPathArgsGlobal,
) {
  const { isMessageEmittable = true } = args;

  if (args.scope === "workspace") {
    const workspaceFolders =
      fLibFs.getWorkspaceFoldersOrShowError(isMessageEmittable);
    if (!workspaceFolders) {
      return;
    }

    const isFleximarkWorkspace = await fLibFs.isFleximarkWorkspace();
    if (!isFleximarkWorkspace) {
      if (isMessageEmittable) {
        vscode.window.showErrorMessage(
          vscode.l10n.t(
            "This command can only be used in a Fleximark workspace.",
          ),
        );
      }
      return;
    }

    const workspacePath = workspaceFolders[0].uri.fsPath;
    return path.join(workspacePath, ".fleximark", "fleximark.css");
  }
  return path.join(args.context.globalStorageUri.fsPath, "fleximark.css");
}

export async function resetGlobalFleximarkCss(
  context: vscode.ExtensionContext,
  isMessageEmittable = true,
) {
  const cssContent = getDefaultFleximarkCss();
  const cssPath = await getCssPath({
    context,
    scope: "global",
    isMessageEmittable,
  });
  if (!cssPath) {
    return;
  }

  try {
    await fs.mkdir(context.globalStorageUri.fsPath, { recursive: true });
    await fs.writeFile(cssPath, cssContent, "utf8");
    if (isMessageEmittable) {
      vscode.window.showErrorMessage(
        vscode.l10n.t("Failed to reset global fleximark.css."),
      );
    }
  } catch {
    if (isMessageEmittable) {
      vscode.window.showInformationMessage(
        vscode.l10n.t("fleximark.css has been saved to global storage."),
      );
    }
  }
}
