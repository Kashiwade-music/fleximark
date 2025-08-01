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

import {
  readGlobalFleximarkCss,
  readWorkspaceFleximarkCss,
} from "../css/index.mjs";
import rehypeLineNumber from "./rehypeLineNumber.mjs";
import rehypeRemovePosition from "./rehypeRemovePosition.mjs";
import remarkDirectiveAdmonitions from "./remarkDirectiveAdmonitions.mjs";
import remarkDirectiveDetails from "./remarkDirectiveDetails.mjs";
import remarkDirectiveTabs from "./remarkDirectiveTabs.mjs";
import remarkYouTube from "./remarkYouTube.mjs";

declare const __DEV__: boolean; // This is set by the esbuild process

interface RenderResult {
  html: string;
  hast: HastRoot;
  mdast: MdastRoot;
  plainText: string;
}

async function renderMarkdownToHtml(
  markdown: string,
  markdownAbsPath: string,
  context: vscode.ExtensionContext,
  webview?: vscode.Webview,
  forExportToFile = false,
  isNeedDataLineNumber = true,
): Promise<RenderResult> {
  // 1. 前半は mdast 生成（remark 部分のみ）
  const remarkProcessor = unified()
    .use(remarkParse)
    .use(remarkFrontmatter)
    .use(remarkYouTube, { mode: webview ? "lazy" : "iframe" })
    .use(remarkGfm)
    .use(remarkMath)
    .use(remarkDirective)
    .use(remarkDirectiveAdmonitions)
    .use(remarkDirectiveDetails)
    .use(remarkDirectiveTabs);

  // parse & transform mdast
  const parsed = remarkProcessor.parse(markdown);
  const mdast = (await remarkProcessor.run(parsed)) as MdastRoot;

  // mdast をここで保存または返却可能
  if (__DEV__) {
    await vscode.workspace.fs.writeFile(
      vscode.Uri.joinPath(context.extensionUri, "debug", "debug-mdast.json"),
      new TextEncoder().encode(JSON.stringify(mdast, null, 2)),
    );
  }

  // 2. 後半は hast 生成（rehype 変換）
  const rehypeProcessor = unified()
    .use(remarkRehype, { allowDangerousHtml: true })
    .use(rehypeKatex)
    .use(rehypePrettyCode, {
      theme: "github-light-default",
      keepBackground: false,
    })
    .use(rehypeRaw)
    .use(rehypeLineNumber, { isNeedDataLineNumber })
    .use(rehypeRemovePosition);

  const hast = await rehypeProcessor.run(mdast);

  // hast をここで保存または返却可能
  if (__DEV__) {
    await vscode.workspace.fs.writeFile(
      vscode.Uri.joinPath(context.extensionUri, "debug", "debug-hast.json"),
      new TextEncoder().encode(JSON.stringify(hast, null, 2)),
    );
  }

  const html = await convertToHtml(
    hast,
    markdownAbsPath,
    context,
    webview,
    forExportToFile,
  );

  return { html, hast, mdast, plainText: markdown };
}

async function convertToHtml(
  hast: HastRoot,
  markdownAbsPath: string,
  context: vscode.ExtensionContext,
  webview?: vscode.Webview,
  forExportToFile?: boolean,
): Promise<string> {
  const htmlProcessor = unified().use(rehypeStringify, {
    allowDangerousHtml: true,
  });
  const htmlBody = String(htmlProcessor.stringify(hast));

  if (webview)
    return wrapHtmlForVscode(htmlBody, markdownAbsPath, context, webview);
  if (forExportToFile) return wrapHtmlForFile(htmlBody, context);

  return wrapHtmlForBrowser(htmlBody, context);
}

// -----------------------------------------------------------------------------
// HTML Wrapping Utilities
// -----------------------------------------------------------------------------

/**
 * Wraps the HTML body in a full document with scripts and styles tailored for direct browser preview.
 *
 * @async
 * @param {string} body - The main HTML body content.
 * @param {vscode.ExtensionContext} context - The extension context used to read configuration and resources.
 * @returns {Promise<string>} A complete HTML document string for browser usage.
 */
async function wrapHtmlForBrowser(
  body: string,
  context: vscode.ExtensionContext,
): Promise<string> {
  const [globalCss, workspaceCss] = await Promise.all([
    readGlobalFleximarkCss(context),
    readWorkspaceFleximarkCss(),
  ]);

  const port =
    (vscode.workspace
      .getConfiguration("fleximark")
      .get<number>("browserPreviewPort") || 3000) + 1;

  const assets = {
    katexCss: "dist/media/katex.min.css",
    abcjsCss: "dist/media/abcjs-audio.css",
    abcjsJs: "dist/media/abcjsScripts.js",
    mermaidJs: "dist/media/mermaidScripts.js",
    webSocketJs: "dist/media/webSocketScripts.js",
    youtubeJs: "dist/media/youtubePlaceholderScripts.js",
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
  <script src="${assets.abcjsJs}"></script>
  <script src="${assets.mermaidJs}"></script>
  <script src="${assets.webSocketJs}"></script>
  <script src="${assets.youtubeJs}"></script>
</head>
<body>
  <div class="markdown-body">${body}</div>
</body>
</html>`;
}

/**
 * Wraps the HTML body in a complete HTML document optimized for exporting to file.
 * This includes inlining styles, embedding scripts, and copying required KaTeX fonts.
 *
 * @async
 * @param {string} body - The HTML body content to wrap.
 * @param {vscode.ExtensionContext} context - The extension context used for locating assets.
 * @returns {Promise<string>} A fully-wrapped HTML document ready for file export.
 */
async function wrapHtmlForFile(
  body: string,
  context: vscode.ExtensionContext,
): Promise<string> {
  const [globalCss, workspaceCss] = await Promise.all([
    readGlobalFleximarkCss(context),
    readWorkspaceFleximarkCss(),
  ]);

  const mediaDir = context.asAbsolutePath("dist/media");
  const readAsset = (file: string) =>
    fs.promises.readFile(path.join(mediaDir, file), "utf8");

  const [abcjsScripts, abcjsCss, mermaidScripts, youtubeScripts] =
    await Promise.all([
      readAsset("abcjsScripts.js"),
      readAsset("abcjs-audio.css"),
      readAsset("mermaidScripts.js"),
      readAsset("youtubePlaceholderScripts.js"),
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
  <script>${abcjsScripts}</script>
  <script>${mermaidScripts}</script>
  <script>${youtubeScripts}</script>
</head>
<body>
  <div class="markdown-body">${body}</div>
</body>
</html>`;
}

function rewriteImgSrcWithWebviewUri(
  html: string,
  markdownAbsPath: string,
  webview: vscode.Webview,
): string {
  return html.replace(
    /<img\s+[^>]*src=["']([^"']+)["'][^>]*>/g,
    (match, src) => {
      try {
        // ローカルファイルパスのみに限定する（file:/// または 絶対パス）
        if (src.startsWith("http") || src.startsWith("data:")) {
          return match; // 外部URLやbase64はそのまま
        }

        console.log(src);

        const localPath = path.isAbsolute(src)
          ? src
          : vscode.Uri.joinPath(
              vscode.Uri.file(path.dirname(markdownAbsPath)),
              src,
            ).fsPath;

        const uri = webview.asWebviewUri(vscode.Uri.file(localPath));
        return match.replace(src, uri.toString(true));
      } catch (err) {
        console.error(`Failed to rewrite img src for: ${src}`, err);
        return match;
      }
    },
  );
}

/**
 * Wraps the HTML body in a secure document for VS Code's Webview API.
 * Applies strict CSP (Content Security Policy) headers and rewrites asset URIs with `asWebviewUri`.
 *
 * @async
 * @param {string} body - The HTML body content to include.
 * @param {string} markdownAbsPath - The absolute path to the Markdown file, used for resolving relative assets.
 * @param {vscode.ExtensionContext} context - VS Code extension context used for resolving resource URIs.
 * @param {vscode.Webview} webview - The active Webview instance used for URI resolution and CSP source generation.
 * @returns {Promise<string>} The fully formed HTML document string for the webview.
 */
async function wrapHtmlForVscode(
  body: string,
  markdownAbsPath: string,
  context: vscode.ExtensionContext,
  webview: vscode.Webview,
): Promise<string> {
  const [globalCss, workspaceCss] = await Promise.all([
    readGlobalFleximarkCss(context),
    readWorkspaceFleximarkCss(),
  ]);

  const getUri = (file: string) =>
    webview.asWebviewUri(
      vscode.Uri.joinPath(context.extensionUri, "dist", "media", file),
    );

  const assets = {
    katexCss: getUri("katex.min.css"),
    abcjsCss: getUri("abcjs-audio.css"),
    abcjsJs: getUri("abcjsScripts.js"),
    mermaidJs: getUri("mermaidScripts.js"),
    vscodeScrollJs: getUri("vscodeWebviewScrollScripts.js"),
    youtubeJs: getUri("youtubePlaceholderScripts.js"),
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
  <script src="${assets.abcjsJs}"></script>
  <script src="${assets.mermaidJs}"></script>
  <script src="${assets.vscodeScrollJs}"></script>
  <script src="${assets.youtubeJs}"></script>
</head>
<body>
  <div class="markdown-body">${rewriteImgSrcWithWebviewUri(
    body,
    markdownAbsPath,
    webview,
  )}</div>
</body>
</html>`;
}

export default renderMarkdownToHtml;
