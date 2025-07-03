import * as vscode from "vscode";
import renderMarkdownToHtml from "./renderMarkdownToHtml/index.mjs";

const exportHtml = async (
  context: vscode.ExtensionContext,
  webview?: vscode.Webview
) => {
  const editor = vscode.window.activeTextEditor;
  const doc = editor?.document;

  if (!editor || !doc || doc.languageId !== "markdown") {
    vscode.window.showErrorMessage(
      "Markdown ファイルがアクティブである必要があります。"
    );
    return;
  }

  if (!webview) {
    vscode.window.showErrorMessage(
      "Webview が利用できません。プレビューを開いてからエクスポートしてください。"
    );
    return;
  }

  try {
    // エクスポート先のパスを決定
    const markdownFilePath = doc.uri.fsPath;
    const htmlFilePath = markdownFilePath.replace(/\.md$/, ".html");

    const uri = vscode.Uri.file(htmlFilePath);
    const enc = new TextEncoder();
    const html = await renderMarkdownToHtml(doc.getText(), context, webview);
    const uint8array = enc.encode(html);

    await vscode.workspace.fs.writeFile(uri, uint8array);
    vscode.window.showInformationMessage(`HTML exported to ${htmlFilePath}`);
  } catch (err) {
    vscode.window.showErrorMessage(`Failed to export HTML: ${err}`);
  }
};

export default exportHtml;
