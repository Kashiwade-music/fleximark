import * as vscode from "vscode";

import * as fLibConvert from "../../lib/convert/index.mjs";

const exportHtml = async (context: vscode.ExtensionContext) => {
  const editor = vscode.window.activeTextEditor;
  const doc = editor?.document;

  if (!editor || !doc || doc.languageId !== "markdown") {
    vscode.window.showErrorMessage(
      vscode.l10n.t("The Markdown file must be active."),
    );
    return;
  }

  try {
    // エクスポート先のパスを決定
    const markdownFilePath = doc.uri.fsPath;
    const htmlFilePath = markdownFilePath.replace(/\.md$/, ".html");

    const uri = vscode.Uri.file(htmlFilePath);
    const enc = new TextEncoder();

    const res = await fLibConvert.convertMdToHtml({
      convertType: "file",
      markdown: doc.getText(),
      context: context,
      isNeedDataLineNumber: false,
    });
    const uint8array = enc.encode(res.html);

    await vscode.workspace.fs.writeFile(uri, uint8array);
    vscode.window.showInformationMessage(
      vscode.l10n.t("HTML exported to {htmlFilePath}", { htmlFilePath }),
    );
  } catch (err) {
    vscode.window.showErrorMessage(
      vscode.l10n.t("Failed to export HTML: {err}", { err: String(err) }),
    );
  }
};

export default exportHtml;
