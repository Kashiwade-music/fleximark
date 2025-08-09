import * as fs from "fs";
import { Root as HastRoot } from "hast";
import { Root as MdastRoot } from "mdast";
import * as path from "path";
import * as vm from "vm";
import * as vscode from "vscode";

import * as fLibFs from "../../fs/index.mjs";

interface Plugin {
  transformMarkdownString: (markdown: string) => string;
  transformMdast: (mdast: MdastRoot) => MdastRoot;
  transformHast: (hast: HastRoot) => HastRoot;
  transformHtmlString: (html: string) => string;
}

export async function loadParserPlugin(
  context: vscode.ExtensionContext,
): Promise<Plugin> {
  const workspacePath = await getPluginPath(context, "workspace");
  const globalPath = await getPluginPath(context, "global");

  let pluginPath: string | undefined;
  if (workspacePath && fs.existsSync(workspacePath)) {
    pluginPath = workspacePath;
  } else if (globalPath && fs.existsSync(globalPath)) {
    pluginPath = globalPath;
  }

  if (!pluginPath) {
    return {
      transformMarkdownString: (markdown) => markdown,
      transformMdast: (mdast) => mdast,
      transformHast: (hast) => hast,
      transformHtmlString: (html) => html,
    };
  }

  const code = await fs.promises.readFile(pluginPath, "utf8");

  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  const sandbox: Record<string, any> = {
    module: { exports: {} },
    exports: {},
  };

  const vmcontext = vm.createContext(sandbox);

  try {
    const script = new vm.Script(code, {
      filename: pluginPath,
    });

    script.runInContext(vmcontext, { timeout: 1000 });

    const plugin = sandbox.module.exports;

    if (
      !plugin ||
      typeof plugin.transformMarkdownString !== "function" ||
      typeof plugin.transformMdast !== "function" ||
      typeof plugin.transformHast !== "function" ||
      typeof plugin.transformHtmlString !== "function"
    ) {
      vscode.window.showWarningMessage(
        vscode.l10n.t(
          "The parser plugin is not valid. Using default functions.",
        ),
      );
      return {
        transformMarkdownString: (markdown) => markdown,
        transformMdast: (mdast) => mdast,
        transformHast: (hast) => hast,
        transformHtmlString: (html) => html,
      };
    }

    // eslint-disable-next-line @typescript-eslint/no-unsafe-function-type, @typescript-eslint/no-explicit-any
    function safeCall(fn: Function, arg: any): any {
      try {
        const serializedArg = JSON.stringify(arg);
        const parsedArg = JSON.parse(serializedArg);

        const result = fn(parsedArg);

        return JSON.parse(JSON.stringify(result));
      } catch (e) {
        console.error("Plugin function call error:", e);
        return arg;
      }
    }

    return {
      transformMarkdownString: (markdown: string) =>
        safeCall(plugin.transformMarkdownString, markdown),
      transformMdast: (mdast: MdastRoot) =>
        safeCall(plugin.transformMdast, mdast),
      transformHast: (hast: HastRoot) => safeCall(plugin.transformHast, hast),
      transformHtmlString: (html: string) =>
        safeCall(plugin.transformHtmlString, html),
    };
  } catch (e) {
    console.error("Plugin load error:", e);
    return {
      transformMarkdownString: (markdown) => markdown,
      transformMdast: (mdast) => mdast,
      transformHast: (hast) => hast,
      transformHtmlString: (html) => html,
    };
  }
}

export async function getPluginPath(
  context: vscode.ExtensionContext,
  scope: "workspace" | "global",
) {
  if (scope === "workspace") {
    const workspaceFolders = fLibFs.getWorkspaceFoldersOrShowError();
    if (!workspaceFolders) {
      return;
    }

    const isFleximarkWorkspace = await fLibFs.isFleximarkWorkspace();
    if (!isFleximarkWorkspace) {
      vscode.window.showErrorMessage(
        vscode.l10n.t(
          "This command can only be used in a Fleximark workspace.",
        ),
      );
      return;
    }

    const workspacePath = workspaceFolders[0].uri.fsPath;
    return path.join(workspacePath, ".fleximark", "parserPlugin.js");
  }
  return path.join(context.globalStorageUri.fsPath, "parserPlugin.js");
}

export async function createParserPluginFile(
  context: vscode.ExtensionContext,
  scope: "workspace" | "global",
) {
  const pluginPath = await getPluginPath(context, scope);
  if (!pluginPath) {
    return;
  }

  const shouldProceed = await fLibFs.confirmOverwriteAndBackupFile(
    vscode.Uri.file(pluginPath),
  );
  if (!shouldProceed) {
    return;
  }

  const defaultContent = `
module.exports = {
  transformMarkdownString(markdown) {
    return markdown;
  },
  transformMdast(mdast) {
    return mdast;
  },
  transformHast(hast) {
    return hast;
  },
  transformHtmlString(html) {
    return html;
  }
};
`.trimStart();

  await fs.promises.writeFile(pluginPath, defaultContent, { encoding: "utf8" });
  return pluginPath;
}
