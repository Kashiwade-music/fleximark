import * as vscode from "vscode";

import * as fLibCss from "../../lib/css/index.mjs";

async function resetGlobalFleximarkCss(
  context: vscode.ExtensionContext,
): Promise<void> {
  await fLibCss.resetGlobalFleximarkCss(context);
}

export default resetGlobalFleximarkCss;
