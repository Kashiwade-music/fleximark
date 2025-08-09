import { Express } from "express";
import * as fs from "fs";
import { Root as HastRoot } from "hast";
import { Root as MdastRoot } from "mdast";
import * as path from "path";
import rehypeKatex from "rehype-katex";
import rehypePrettyCode from "rehype-pretty-code";
import rehypeRaw from "rehype-raw";
import rehypeStringify from "rehype-stringify";
import remarkDirective from "remark-directive";
import remarkFrontmatter from "remark-frontmatter";
import remarkGfm from "remark-gfm";
import remarkMath from "remark-math";
import remarkParse from "remark-parse";
import remarkRehype from "remark-rehype";
import { unified } from "unified";
import * as vscode from "vscode";

import * as fLibCss from "../../css/index.mjs";
import * as fLibConvertPlugin from "../plugin/index.mjs";
import rehypeLineNumber from "./rehypeLineNumber.mjs";
import rehypeLocalSrcConvert, {
  RehypeLocalSrcConvertArgs,
} from "./rehypeLocalSrcConvert.mjs";
import rehypeRemovePosition from "./rehypeRemovePosition.mjs";
import remarkDirectiveAdmonitions from "./remarkDirectiveAdmonitions.mjs";
import remarkDirectiveDetails from "./remarkDirectiveDetails.mjs";
import remarkDirectiveTabs from "./remarkDirectiveTabs.mjs";
import remarkYouTube from "./remarkYouTube.mjs";

declare const __DEV__: boolean; // This is set by the esbuild process

export interface ConvertResult {
  html: string;
  hast: HastRoot;
  mdast: MdastRoot;
  plainText: string;
}

interface BaseArgs {
  markdown: string;
  context: vscode.ExtensionContext;
  isNeedDataLineNumber?: boolean; // Optional, defaults to true
}

interface WebviewArgs extends BaseArgs {
  convertType: "webview";
  webview: vscode.Webview;
  markdownAbsPath: string;
}

interface BrowserArgs extends BaseArgs {
  convertType: "browser";
  app: Express;
  markdownAbsPath: string;
}

interface FileArgs extends BaseArgs {
  convertType: "file";
}

type ConvertArgs = WebviewArgs | BrowserArgs | FileArgs;

export async function convertMdToHtml(
  args: ConvertArgs,
): Promise<ConvertResult> {
  args.isNeedDataLineNumber ??= true; // Default to true if not provided
  const plugin = await fLibConvertPlugin.loadParserPlugin(args.context);

  args.markdown = plugin.transformMarkdownString(args.markdown);

  let mdast = await toMdastFromMarkdown(args);
  mdast = plugin.transformMdast(mdast);
  if (__DEV__) await writeDebugJson("debug-mdast.json", mdast, args.context);

  let hast = await toHastFromMdast(mdast, args);
  hast = plugin.transformHast(hast);
  if (__DEV__) await writeDebugJson("debug-hast.json", hast, args.context);

  let htmlBody = await toHtmlBodyFromHast(hast);
  htmlBody = plugin.transformHtmlString(htmlBody);

  const html = await wrapHtml(htmlBody, args);

  return { html, hast, mdast, plainText: args.markdown };
}

async function toMdastFromMarkdown(args: ConvertArgs): Promise<MdastRoot> {
  const processor = unified()
    .use(remarkParse)
    .use(remarkFrontmatter)
    .use(remarkYouTube, {
      mode: args.convertType === "webview" ? "lazy" : "iframe",
    })
    .use(remarkGfm)
    .use(remarkMath)
    .use(remarkDirective)
    .use(remarkDirectiveAdmonitions)
    .use(remarkDirectiveDetails)
    .use(remarkDirectiveTabs);

  const parsed = processor.parse(args.markdown);
  return processor.run(parsed) as Promise<MdastRoot>;
}

async function toHastFromMdast(
  mdast: MdastRoot,
  args: ConvertArgs,
): Promise<HastRoot> {
  let rehypeLocalSrcConvertArgs: RehypeLocalSrcConvertArgs;
  switch (args.convertType) {
    case "webview":
      rehypeLocalSrcConvertArgs = {
        convertType: "webview",
        markdownAbsPath: args.markdownAbsPath,
        webview: args.webview,
      };
      break;
    case "browser":
      rehypeLocalSrcConvertArgs = {
        convertType: "browser",
        markdownAbsPath: args.markdownAbsPath,
        app: args.app,
      };
      break;
    case "file":
      rehypeLocalSrcConvertArgs = { convertType: "file" };
      break;
    default:
      throw new Error(`Unhandled convertType: ${args}`);
  }

  const processor = unified()
    .use(remarkRehype, { allowDangerousHtml: true })
    .use(rehypeKatex)
    .use(rehypePrettyCode, {
      theme: "github-light-default",
      keepBackground: false,
    })
    .use(rehypeRaw)
    .use(rehypeLineNumber, { isNeedDataLineNumber: args.isNeedDataLineNumber })
    .use(rehypeRemovePosition)
    .use(rehypeLocalSrcConvert, rehypeLocalSrcConvertArgs);

  return (await processor.run(mdast)) as HastRoot;
}

async function toHtmlBodyFromHast(hast: HastRoot): Promise<string> {
  return unified()
    .use(rehypeStringify, { allowDangerousHtml: true })
    .stringify(hast) as string;
}

async function wrapHtml(htmlBody: string, args: ConvertArgs): Promise<string> {
  switch (args.convertType) {
    case "webview":
      return wrapHtmlForWebview(
        htmlBody,
        args.markdownAbsPath,
        args.context,
        args.webview,
      );
    case "browser":
      return wrapHtmlForBrowser(htmlBody, args.context);
    case "file":
      return wrapHtmlForFile(htmlBody, args.context);
    default:
      throw new Error(`Unhandled convertType: ${args}`);
  }
}

async function writeDebugJson(
  fileName: string,
  data: unknown,
  context: vscode.ExtensionContext,
): Promise<void> {
  const debugPath = vscode.Uri.joinPath(
    context.extensionUri,
    "debug",
    fileName,
  );
  const content = new TextEncoder().encode(JSON.stringify(data, null, 2));
  await vscode.workspace.fs.writeFile(debugPath, content);
}

// -----------------------------------------------------------------------------
// HTML Wrapping Utilities
// -----------------------------------------------------------------------------

async function wrapHtmlForBrowser(
  body: string,
  context: vscode.ExtensionContext,
): Promise<string> {
  const [globalCss, workspaceCss] = await Promise.all([
    fLibCss.readGlobalFleximarkCss(context),
    fLibCss.readWorkspaceFleximarkCss(),
  ]);

  const port =
    (vscode.workspace
      .getConfiguration("fleximark")
      .get<number>("browserPreviewPort") || 3000) + 1;

  const assets = {
    katexCss: "dist/media/katex.min.css",
    abcjsCss: "dist/media/abcjs-audio.css",
    browserJs: "dist/media/browser.js",
  };

  return `
<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="UTF-8" />
  <title>Preview</title>
  <link rel="stylesheet" href="${assets.katexCss}" />
  <link rel="stylesheet" href="${assets.abcjsCss}" />
  <style>${globalCss}${workspaceCss}</style>
  <script>window.webSocketUrl = "ws://localhost:${port}";</script>
  <script src="${assets.browserJs}"></script>
</head>
<body>
  <div class="markdown-body">${body}</div>
</body>
</html>`;
}

async function wrapHtmlForFile(
  body: string,
  context: vscode.ExtensionContext,
): Promise<string> {
  const [globalCss, workspaceCss] = await Promise.all([
    fLibCss.readGlobalFleximarkCss(context),
    fLibCss.readWorkspaceFleximarkCss(),
  ]);

  const mediaDir = context.asAbsolutePath("dist/media");
  const readAsset = (file: string) =>
    fs.promises.readFile(path.join(mediaDir, file), "utf8");

  const [abcjsCss, fileJs] = await Promise.all([
    readAsset("abcjs-audio.css"),
    readAsset("file.js"),
  ]);

  const katexCssUri = vscode.Uri.joinPath(
    context.globalStorageUri,
    "katex.min.css",
  );
  const katexFontsUri = vscode.Uri.joinPath(context.globalStorageUri, "fonts");

  await vscode.workspace.fs.copy(
    vscode.Uri.joinPath(context.extensionUri, "dist", "media", "katex.min.css"),
    katexCssUri,
    { overwrite: true },
  );

  await vscode.workspace.fs.copy(
    vscode.Uri.joinPath(context.extensionUri, "dist", "media", "fonts"),
    katexFontsUri,
    { overwrite: true },
  );

  return `
<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="UTF-8" />
  <title>Preview</title>
  <link rel="stylesheet" href="file://${katexCssUri.fsPath}" />
  <style>${abcjsCss}${globalCss}${workspaceCss}</style>
  <script>${fileJs}</script>
</head>
<body>
  <div class="markdown-body">${body}</div>
</body>
</html>`;
}

async function wrapHtmlForWebview(
  body: string,
  markdownAbsPath: string,
  context: vscode.ExtensionContext,
  webview: vscode.Webview,
): Promise<string> {
  const [globalCss, workspaceCss] = await Promise.all([
    fLibCss.readGlobalFleximarkCss(context),
    fLibCss.readWorkspaceFleximarkCss(),
  ]);

  const getUri = (file: string) =>
    webview.asWebviewUri(
      vscode.Uri.joinPath(context.extensionUri, "dist", "media", file),
    );

  const assets = {
    katexCss: getUri("katex.min.css"),
    abcjsCss: getUri("abcjs-audio.css"),
    webviewJs: getUri("webview.js"),
  };

  return `
<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="UTF-8" />
  <title>Preview</title>
  <meta http-equiv="Content-Security-Policy"
        content="default-src 'none';
                 img-src ${webview.cspSource} https:;
                 script-src ${webview.cspSource};
                 font-src ${webview.cspSource};
                 frame-src https://www.youtube.com https://www.youtube-nocookie.com;
                 style-src ${webview.cspSource} 'unsafe-inline' https:;
                 connect-src https://paulrosen.github.io;" />
  <link rel="stylesheet" href="${assets.katexCss}" />
  <link rel="stylesheet" href="${assets.abcjsCss}" />
  <style>${globalCss}${workspaceCss}</style>
  <script src="${assets.webviewJs}"></script>
</head>
<body>
  <div class="markdown-body">${body}</div>
</body>
</html>`;
}
