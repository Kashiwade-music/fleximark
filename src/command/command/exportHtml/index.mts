import * as path from "path";
import * as vscode from "vscode";

import * as fLibConvert from "../../lib/convert/convertMdToHtml/index.mjs";

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
    const markdownFilePath = doc.uri.fsPath;
    const markdownDir = path.dirname(markdownFilePath);
    const markdownBaseName = path.basename(markdownFilePath, ".md");

    const exportDir = path.join(markdownDir, markdownBaseName);

    const exportDirUri = vscode.Uri.file(exportDir);
    try {
      await vscode.workspace.fs.stat(exportDirUri);

      // delete existing contents
      const files = await vscode.workspace.fs.readDirectory(exportDirUri);
      for (const [fileName] of files) {
        await vscode.workspace.fs.delete(
          vscode.Uri.joinPath(exportDirUri, fileName),
          { recursive: true, useTrash: true },
        );
      }
    } catch {
      await vscode.workspace.fs.createDirectory(exportDirUri);
    }

    const htmlFilePath = path.join(exportDir, `${markdownBaseName}.html`);
    const htmlFileUri = vscode.Uri.file(htmlFilePath);

    const enc = new TextEncoder();

    const res = await fLibConvert.convertMdToHtml({
      convertType: "file",
      markdown: doc.getText(),
      markdownAbsPath: markdownFilePath,
      context: context,
      isNeedDataLineNumber: false,
      distDir: exportDir,
    });
    const uint8array = enc.encode(res.html);

    await vscode.workspace.fs.writeFile(htmlFileUri, uint8array);
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
