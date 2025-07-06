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
import remarkYouTube from "./reamarkYouTube.mjs";
import rehypeRemovePosition from "./rehypeRemovePosition.mjs";

import {
  readGlobalMarknoteCss,
  readWorkspaceMarknoteCss,
} from "../css/index.mjs";

type RenderResult = {
  html: string;
  hast: Root;
};

export const renderMarkdownToHtml = async (
  markdown: string,
  context: vscode.ExtensionContext,
  webview?: vscode.Webview,
  forExportToFile = false
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

// -- Wrapping Functions -------------------------------------------------------

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
