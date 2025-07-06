import * as vscode from "vscode";
import { unified } from "unified";
import { Root } from "hast";
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
import rehypeRemovePosition from "./rehypeRemovePosition.mjs";
import * as path from "path";
import * as fs from "fs";

export const renderMarkdownToHtml = async (
  markdown: string,
  context: vscode.ExtensionContext,
  webview?: vscode.Webview,
  forExportToFile: boolean = false
): Promise<{ html: string; hast: Root }> => {
  const processor = unified()
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
    .use(rehypeRemovePosition);

  // Markdown を中間AST (HAST) まで変換
  const hast = (await processor.run(processor.parse(markdown))) as Root;

  // output as plaintext file for debugging
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

  // HTML に変換
  const htmlProcessor = processor().use(rehypeStringify, {
    allowDangerousHtml: true,
  });

  const htmlString = String(htmlProcessor.stringify(hast));

  const html = webview
    ? await wrapHtmlForVscode(htmlString, context, webview)
    : forExportToFile
    ? await wrapHtmlForFile(htmlString, context)
    : await wrapHtmlForBrowser(htmlString, context);

  return {
    html,
    hast,
  };
};

const wrapHtmlForBrowser = async (
  body: string,
  context: vscode.ExtensionContext
): Promise<string> => {
  const globalCss = await readGlobalMarknoteCss(context);
  const workspaceCss = await readWorkspaceMarknoteCss();

  const abcjsScriptsUri = "dist/media/abcjsScripts.js";
  const abdjsCssUri = "dist/media/abcjs-audio.css";

  const katexCssUri = "dist/media/katex.min.css";

  const mermaidScriptsUri = "dist/media/mermaidScripts.js";

  const webSocketScriptsUri = "dist/media/webSocketScripts.js";

  const youtubePlaceholderScriptsUri =
    "dist/media/youtubePlaceholderScripts.js";

  const port =
    (vscode.workspace
      .getConfiguration("marknote")
      .get<number>("browserPreviewPort") || 3000) + 1;

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
  <script>
    window.webSocketUrl = "ws://localhost:${port}";
  </script>

  <script src="${abcjsScriptsUri}"></script>
  <script src="${mermaidScriptsUri}"></script>
  <script src="${webSocketScriptsUri}"></script>
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

const wrapHtmlForFile = async (
  body: string,
  context: vscode.ExtensionContext
): Promise<string> => {
  const globalCss = await readGlobalMarknoteCss(context);
  const workspaceCss = await readWorkspaceMarknoteCss();

  // 各ファイルのパスを解決して中身を読み込む
  const mediaPath = context.asAbsolutePath("dist/media");

  const readFileAsString = async (relativePath: string) => {
    const absolutePath = path.join(mediaPath, relativePath);
    return fs.promises.readFile(absolutePath, "utf8");
  };

  const abcjsScripts = await readFileAsString("abcjsScripts.js");
  const abdjsCss = await readFileAsString("abcjs-audio.css");
  const mermaidScripts = await readFileAsString("mermaidScripts.js");
  const youtubePlaceholderScripts = await readFileAsString(
    "youtubePlaceholderScripts.js"
  );

  // KaTeXについてはCSSとFontファイルをcontext.globalStorageUriにコピー
  // dist\media\katex.min.css -> context.globalStorageUri\katex.min.css
  // dist\media\fonts -> context.globalStorageUri\fonts

  vscode.workspace.fs.copy(
    vscode.Uri.joinPath(context.extensionUri, "dist", "media", "katex.min.css"),
    vscode.Uri.joinPath(context.globalStorageUri, "katex.min.css"),
    { overwrite: true }
  );

  vscode.workspace.fs.copy(
    vscode.Uri.joinPath(context.extensionUri, "dist", "media", "fonts"),
    vscode.Uri.joinPath(context.globalStorageUri, "fonts"),
    { overwrite: true }
  );

  return `
<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="UTF-8">
  <title>Preview</title>

  <link rel="stylesheet" href="file://${context.globalStorageUri.fsPath}/katex.min.css">

  <style>
    ${abdjsCss}
    ${globalCss}
    ${workspaceCss}
  </style>

  <script>
    ${abcjsScripts}
  </script>
  <script>
    ${mermaidScripts}
  </script>
  <script>
    ${youtubePlaceholderScripts}
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

const wrapHtmlForVscode = async (
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
