import * as fs from "fs/promises";
import path from "path";
import * as vscode from "vscode";

import * as fLibCss from "../../lib/css/index.mjs";
import * as fLibFs from "../../lib/fs/index.mjs";

export default async function openFleximarkCss(
  context: vscode.ExtensionContext,
  scope: "workspace" | "global",
): Promise<void> {
  try {
    if (scope === "workspace") {
      await openWorkspaceCss(context);
    } else {
      await openGlobalCss(context);
    }
  } catch (error) {
    vscode.window.showErrorMessage(
      vscode.l10n.t(
        "An unexpected error occurred while opening Fleximark CSS.",
      ),
    );
    console.error(error);
  }
}

async function openWorkspaceCss(
  context: vscode.ExtensionContext,
): Promise<void> {
  const cssPath = await fLibCss.getCssPath({
    scope: "workspace",
    isMessageEmittable: true,
  });
  if (!cssPath) return;

  const cssUri = vscode.Uri.file(cssPath);

  if (await fLibFs.isFileExists(cssUri)) {
    await openDocument(cssUri);
    return;
  }

  const globalCssPath = await fLibCss.getCssPath({
    scope: "global",
    context,
    isMessageEmittable: true,
  });
  if (!globalCssPath) return;

  const defaultContent = `/* 
 * This file is used to override default styles.
 * Default styles are located at:
 * ${globalCssPath}
 * 
 * Please edit this file to customize Fleximark appearance.
 */`;

  try {
    await fs.mkdir(path.dirname(cssPath), { recursive: true });
    await fs.writeFile(cssPath, defaultContent, "utf8");

    vscode.window.showInformationMessage(
      vscode.l10n.t(
        "Created .fleximark directory and fleximark.css file in the workspace.",
      ),
    );

    await openDocument(cssUri);
  } catch (error) {
    vscode.window.showErrorMessage(
      vscode.l10n.t("Failed to create the workspace fleximark.css file."),
    );
    console.error(error);
  }
}

async function openGlobalCss(context: vscode.ExtensionContext): Promise<void> {
  const cssPath = await fLibCss.getCssPath({
    scope: "global",
    context,
    isMessageEmittable: true,
  });
  if (!cssPath) return;

  const cssUri = vscode.Uri.file(cssPath);

  if (!(await fLibFs.isFileExists(cssUri))) {
    await fLibCss.resetGlobalFleximarkCss(context);
  }

  await openDocument(cssUri);
}

async function openDocument(uri: vscode.Uri): Promise<void> {
  const document = await vscode.workspace.openTextDocument(uri);
  await vscode.window.showTextDocument(document);
}
