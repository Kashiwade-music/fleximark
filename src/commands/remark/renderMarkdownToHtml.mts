import * as fs from "fs";
import * as path from "path";
import * as vscode from "vscode";
import { unified } from "unified";
import remarkParse from "remark-parse";
import remarkGfm from "remark-gfm";
import remarkMath from "remark-math";
import remarkRehype from "remark-rehype";
import rehypeKatex from "rehype-katex";
import rehypePrettyCode from "rehype-pretty-code";
import rehypeStringify from "rehype-stringify";

const renderMarkdownToHtml = async (markdown: string): Promise<string> => {
  const file = await unified()
    .use(remarkParse)
    .use(remarkGfm)
    .use(remarkMath)
    .use(remarkRehype, { allowDangerousHtml: true })
    .use(rehypeKatex)
    .use(rehypePrettyCode, {
      theme: "github-dark-default",
      keepBackground: true,
    })
    .use(rehypeStringify, { allowDangerousHtml: true })
    .process(markdown);

  return wrapHtml(String(file));
};

const wrapHtml = async (body: string): Promise<string> => {
  const globalStoragePath = path.join(
    vscode.env.appRoot,
    "User",
    "globalStorage",
    "marknote.css"
  );

  const workspaceFolder = vscode.workspace.workspaceFolders?.[0]?.uri.fsPath;
  const workspaceCssPath = workspaceFolder
    ? path.join(workspaceFolder, ".marknote", "marknote.css")
    : null;

  const readCss = (filePath: string | null): string => {
    if (filePath && fs.existsSync(filePath)) {
      return fs.readFileSync(filePath, "utf-8");
    }
    return "";
  };

  const globalCss = readCss(globalStoragePath);
  const workspaceCss = readCss(workspaceCssPath);

  return `
<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="UTF-8">
  <title>Preview</title>
  <link rel="stylesheet" href="https://cdn.jsdelivr.net/npm/katex@0.16.8/dist/katex.min.css">
  <style>
    ${globalCss}
    ${workspaceCss}
  </style>
</head>
<body>
  ${body}
  <script>
    console.log("Markdown Preview loaded");
  </script>
</body>
</html>
`;
};

export default renderMarkdownToHtml;
