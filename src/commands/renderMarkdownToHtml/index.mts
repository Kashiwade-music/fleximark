import * as vscode from "vscode";
import { unified } from "unified";
import remarkParse from "remark-parse";
import remarkGfm from "remark-gfm";
import remarkMath from "remark-math";
import remarkRehype from "remark-rehype";
import rehypeKatex from "rehype-katex";
import rehypePrettyCode from "rehype-pretty-code";
import rehypeStringify from "rehype-stringify";
import {
  readGlobalMarknoteCss,
  readWorkspaceMarknoteCss,
} from "../css/index.mjs";
import remarkDirective from "remark-directive";
import remarkDirectiveAdmonitions from "./remarkDirectiveAdmonitions.mjs";
import remarkDirectiveDetails from "./remarkDirectiveDetails.mjs";
import remarkDirectiveTabs from "./remarkDirectiveTabs.mjs";
import remarkYouTube from "./reamarkYouTube.mjs";

const renderMarkdownToHtml = async (
  markdown: string,
  context: vscode.ExtensionContext,
  webview?: vscode.Webview
): Promise<string> => {
  const file = await unified()
    .use(remarkParse)
    .use(remarkYouTube)
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
    .use(rehypeStringify, { allowDangerousHtml: true })
    .process(markdown);

  if (!webview) {
    return wrapHtml(String(file), context);
  } else {
    return wrapHtmlWithVscodeUri(String(file), context, webview);
  }
};

const wrapHtml = async (
  body: string,
  context: vscode.ExtensionContext
): Promise<string> => {
  const globalCss = await readGlobalMarknoteCss(context);
  const workspaceCss = await readWorkspaceMarknoteCss();

  const abcjsScriptsUri = "dist/media/abcjsScripts.js";
  const abdjsCssUri = "dist/media/abcjs-audio.css";

  const katexCssUri = "dist/media/katex.min.css";

  const mermaidScriptsUri = "dist/media/mermaidScripts.js";

  const youtubePlaceholderScriptsUri =
    "dist/media/youtubePlaceholderScripts.js";

  return `
<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="UTF-8">
  <title>Preview</title>

  <link rel="stylesheet" href="${katexCssUri}">
  <link rel="stylesheet" href="${abdjsCssUri}">
  <style>
    ${globalCss}
    ${workspaceCss}
  </style>
  <script src="${abcjsScriptsUri}"></script>
  <script src="${mermaidScriptsUri}"></script>
  <script src="${youtubePlaceholderScriptsUri}"></script>
</head>
<body>
  <div class="markdown-body">
    ${body}
  </div>
</body>
</html>
`;
};

const wrapHtmlWithVscodeUri = async (
  body: string,
  context: vscode.ExtensionContext,
  webview: vscode.Webview
): Promise<string> => {
  const globalCss = await readGlobalMarknoteCss(context);
  const workspaceCss = await readWorkspaceMarknoteCss();

  const abcjsScriptsUri = webview.asWebviewUri(
    vscode.Uri.joinPath(
      context.extensionUri,
      "dist",
      "media",
      "abcjsScripts.js"
    )
  );
  const abdjsCssUri = webview.asWebviewUri(
    vscode.Uri.joinPath(
      context.extensionUri,
      "dist",
      "media",
      "abcjs-audio.css"
    )
  );

  const katexCssUri = webview.asWebviewUri(
    vscode.Uri.joinPath(context.extensionUri, "dist", "media", "katex.min.css")
  );

  const mermaidScriptsUri = webview.asWebviewUri(
    vscode.Uri.joinPath(
      context.extensionUri,
      "dist",
      "media",
      "mermaidScripts.js"
    )
  );

  const youtubePlaceholderScriptsUri = webview.asWebviewUri(
    vscode.Uri.joinPath(
      context.extensionUri,
      "dist",
      "media",
      "youtubePlaceholderScripts.js"
    )
  );

  return `
<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="UTF-8">
  <title>Preview</title>

  <meta
    http-equiv="Content-Security-Policy"
    content="default-src 'none'; 
            img-src ${webview.cspSource} https:;
            script-src ${webview.cspSource};
            font-src ${webview.cspSource};
            frame-src https://www.youtube.com https://www.youtube-nocookie.com;
            style-src ${webview.cspSource} 'unsafe-inline' https:;
            connect-src https://paulrosen.github.io;"
  />

  <link rel="stylesheet" href="${katexCssUri}">
  <link rel="stylesheet" href="${abdjsCssUri}">
  <style>
    ${globalCss}
    ${workspaceCss}
  </style>
  <script src="${abcjsScriptsUri}"></script>
  <script src="${mermaidScriptsUri}"></script>
  <script src="${youtubePlaceholderScriptsUri}"></script>
</head>
<body>
  <div class="markdown-body">
    ${body}
  </div>
</body>
</html>
`;
};

export default renderMarkdownToHtml;
