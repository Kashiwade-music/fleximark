import * as vscode from "vscode";
import * as path from "path";
import * as fs from "fs";

import { unified } from "unified";
import { Root } from "hast";
import remarkParse from "remark-parse";
import remarkGfm from "remark-gfm";
import remarkMath from "remark-math";
import remarkRehype from "remark-rehype";
import rehypeKatex from "rehype-katex";
import rehypePrettyCode from "rehype-pretty-code";
import rehypeStringify from "rehype-stringify";

import remarkDirective from "remark-directive";
import remarkDirectiveAdmonitions from "./remarkDirectiveAdmonitions.mjs";
import remarkDirectiveDetails from "./remarkDirectiveDetails.mjs";
import remarkDirectiveTabs from "./remarkDirectiveTabs.mjs";
import remarkLineNumber from "./remarkLineNumber.mjs";
import remarkYouTube from "./remarkYouTube.mjs";
import rehypeRemovePosition from "./rehypeRemovePosition.mjs";

import {
  readGlobalMarknoteCss,
  readWorkspaceMarknoteCss,
} from "../css/index.mjs";

type RenderResult = {
  html: string;
  hast: Root;
};

/**
 * Renders Markdown input into HTML, producing both a HAST (Hypertext Abstract Syntax Tree)
 * and final HTML output. Supports extended syntax like GitHub-flavored Markdown (GFM),
 * math via KaTeX, custom directives (admonitions, tabs, details), YouTube embeds,
 * and syntax highlighting.
 *
 * @param markdown - The raw Markdown string to be rendered.
 * @param context - VS Code extension context for accessing resources and workspace.
 * @param webview - Optional webview object used when rendering HTML for VS Code preview.
 * @param forExportToFile - Flag indicating whether the rendering is intended for file export.
 * @returns A Promise resolving to an object containing both rendered HTML and HAST.
 */
export const renderMarkdownToHtml = async (
  markdown: string,
  context: vscode.ExtensionContext,
  webview?: vscode.Webview,
  forExportToFile = false,
  isNeedDataLineNumber = true
): Promise<RenderResult> => {
  const processor = unified()
    .use(remarkParse)
    .use(remarkYouTube, { mode: webview ? "lazy" : "iframe" })
    .use(remarkGfm)
    .use(remarkMath)
    .use(remarkDirective)
    .use(remarkDirectiveAdmonitions)
    .use(remarkDirectiveDetails)
    .use(remarkDirectiveTabs)
    .use(remarkLineNumber, { isNeedDataLineNumber })
    .use(remarkRehype, { allowDangerousHtml: true })
    .use(rehypeKatex)
    .use(rehypePrettyCode, {
      theme: "github-light-default",
      keepBackground: false,
    })
    .use(rehypeRemovePosition);

  const hast = (await processor.run(processor.parse(markdown))) as Root;

  // Debug output for inspection
  const debugFilePath = vscode.Uri.joinPath(
    context.extensionUri,
    "dist",
    "debug",
    "debug-hast.json"
  );
  await vscode.workspace.fs.writeFile(
    debugFilePath,
    new TextEncoder().encode(JSON.stringify(hast, null, 2))
  );

  // Convert HAST to HTML
  const html = await convertToHtml(hast, context, webview, forExportToFile);
  return { html, hast };
};

/**
 * Converts a given HAST tree into a final HTML string.
 * Depending on context, wraps the HTML appropriately for:
 * - VS Code webview
 * - File export (with assets)
 * - Browser preview
 *
 * @param hast - The HAST object to stringify and wrap.
 * @param context - The extension context, used to access resources.
 * @param webview - Optional webview instance, if rendering inside VS Code.
 * @param forExportToFile - Optional flag for export-specific output.
 * @returns A Promise resolving to the final HTML string.
 */
async function convertToHtml(
  hast: Root,
  context: vscode.ExtensionContext,
  webview?: vscode.Webview,
  forExportToFile?: boolean
): Promise<string> {
  const htmlProcessor = unified().use(rehypeStringify, {
    allowDangerousHtml: true,
  });

  const htmlBody = String(htmlProcessor.stringify(hast));

  if (webview) {
    return wrapHtmlForVscode(htmlBody, context, webview);
  }

  return forExportToFile
    ? wrapHtmlForFile(htmlBody, context)
    : wrapHtmlForBrowser(htmlBody, context);
}

// -----------------------------------------------------------------------------
// Wrapping Functions
// -----------------------------------------------------------------------------

/**
 * Wraps HTML content with required scripts and styles for in-browser preview.
 * Includes CSS from global and workspace config and embeds required scripts.
 *
 * @param body - The HTML body content to embed.
 * @param context - Extension context to read configuration and resources.
 * @returns A Promise resolving to the complete HTML document as a string.
 */
async function wrapHtmlForBrowser(
  body: string,
  context: vscode.ExtensionContext
): Promise<string> {
  const [globalCss, workspaceCss] = await Promise.all([
    readGlobalMarknoteCss(context),
    readWorkspaceMarknoteCss(),
  ]);

  const port =
    (vscode.workspace
      .getConfiguration("marknote")
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
  <style>
    ${globalCss}
    ${workspaceCss}
  </style>
  <script>
    window.webSocketUrl = "ws://localhost:${port}";
  </script>
  <script src="${assets.abcjsJs}"></script>
  <script src="${assets.mermaidJs}"></script>
  <script src="${assets.webSocketJs}"></script>
  <script src="${assets.youtubeJs}"></script>
</head>
<body>
  <div class="markdown-body">
    ${body}
  </div>
</body>
</html>
`;
}

/**
 * Wraps HTML body content for file export. Includes inline CSS and JS,
 * and copies necessary assets (KaTeX fonts, styles) to the global storage path.
 *
 * @param body - The HTML content to wrap.
 * @param context - Extension context for accessing assets and writing files.
 * @returns A Promise resolving to the complete file-ready HTML string.
 */
async function wrapHtmlForFile(
  body: string,
  context: vscode.ExtensionContext
): Promise<string> {
  const [globalCss, workspaceCss] = await Promise.all([
    readGlobalMarknoteCss(context),
    readWorkspaceMarknoteCss(),
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

  // Copy KaTeX resources to storage
  const katexCssUri = vscode.Uri.joinPath(
    context.globalStorageUri,
    "katex.min.css"
  );
  const katexFontsUri = vscode.Uri.joinPath(context.globalStorageUri, "fonts");

  await vscode.workspace.fs.copy(
    vscode.Uri.joinPath(context.extensionUri, "dist", "media", "katex.min.css"),
    katexCssUri,
    { overwrite: true }
  );

  await vscode.workspace.fs.copy(
    vscode.Uri.joinPath(context.extensionUri, "dist", "media", "fonts"),
    katexFontsUri,
    { overwrite: true }
  );

  return `
<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="UTF-8" />
  <title>Preview</title>
  <link rel="stylesheet" href="file://${katexCssUri.fsPath}" />
  <style>
    ${abcjsCss}
    ${globalCss}
    ${workspaceCss}
  </style>
  <script>${abcjsScripts}</script>
  <script>${mermaidScripts}</script>
  <script>${youtubeScripts}</script>
</head>
<body>
  <div class="markdown-body">
    ${body}
  </div>
</body>
</html>
`;
}

/**
 * Wraps HTML content for use inside a VS Code Webview.
 * Applies CSP (Content Security Policy), sets up resource URIs,
 * and injects required JS/CSS for enhanced Markdown features.
 *
 * @param body - The raw HTML content.
 * @param context - Extension context to resolve asset paths.
 * @param webview - VS Code Webview for URI resolution and CSP.
 * @returns A complete HTML document string tailored for Webview usage.
 */
async function wrapHtmlForVscode(
  body: string,
  context: vscode.ExtensionContext,
  webview: vscode.Webview
): Promise<string> {
  const [globalCss, workspaceCss] = await Promise.all([
    readGlobalMarknoteCss(context),
    readWorkspaceMarknoteCss(),
  ]);

  const resourceUri = (file: string) =>
    webview.asWebviewUri(
      vscode.Uri.joinPath(context.extensionUri, "dist", "media", file)
    );

  const assets = {
    katexCss: resourceUri("katex.min.css"),
    abcjsCss: resourceUri("abcjs-audio.css"),
    abcjsJs: resourceUri("abcjsScripts.js"),
    mermaidJs: resourceUri("mermaidScripts.js"),
    vscodeScrollJs: resourceUri("vscodeWebviewScrollScripts.js"),
    youtubeJs: resourceUri("youtubePlaceholderScripts.js"),
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
  <style>
    ${globalCss}
    ${workspaceCss}
  </style>
  <script src="${assets.abcjsJs}"></script>
  <script src="${assets.mermaidJs}"></script>
  <script src="${assets.vscodeScrollJs}"></script>
  <script src="${assets.youtubeJs}"></script>
</head>
<body>
  <div class="markdown-body">
    ${body}
  </div>
</body>
</html>
`;
}

export default renderMarkdownToHtml;
