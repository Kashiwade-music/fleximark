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
  webview: vscode.Webview
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

  return wrapHtml(String(file), context, webview);
};

const wrapHtml = async (
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
      "abcjs-audio.min.css"
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

  <script>
    document.addEventListener("click", (e) => {
      const placeholder = e.target.closest(".youtube-placeholder");
      if (!placeholder) return;

      const videoId = placeholder.dataset.videoId;
      const iframe = document.createElement("iframe");
      iframe.src = "https://www.youtube.com/embed/" + videoId + "?autoplay=1";
      iframe.style = "position: absolute; top: 0; left: 0; width: 100%; height: 100%;";
      iframe.setAttribute("frameborder", "0");
      iframe.setAttribute("allow", "accelerometer; autoplay; clipboard-write; encrypted-media; gyroscope; picture-in-picture");
      iframe.setAttribute("allowfullscreen", "true");

      placeholder.innerHTML = "";
      placeholder.appendChild(iframe);
    });
  </script>
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
