import * as fs from "fs";
import * as vscode from "vscode";

import * as fLibConvert from "../../lib/convert/index.mjs";

async function openParserPluginFile(
  context: vscode.ExtensionContext,
  scope: "workspace" | "global",
) {
  const pluginPath = await fLibConvert.getPluginPath(context, scope);
  if (!pluginPath) {
    return;
  }

  if (!fs.existsSync(pluginPath)) {
    await fLibConvert.createParserPluginFile(context, scope);
  }

  const doc = await vscode.workspace.openTextDocument(pluginPath);
  await vscode.window.showTextDocument(doc, { preview: false });
}

export default openParserPluginFile;
