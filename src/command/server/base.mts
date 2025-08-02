import { Root as HastRoot } from "hast";
import { Root as MdastRoot } from "mdast";
import * as vscode from "vscode";

import * as fLibConvert from "../lib/convert/index.mjs";
import * as fLibUnist from "../lib/unist/index.mjs";

interface ScrollMessage {
  type: "editor-scroll";
  line: number;
}

abstract class BaseServer {
  html?: string;
  hast?: HastRoot;
  mdast?: MdastRoot;
  plainText?: string;
  editorPanel?: vscode.TextEditor;
  isScrollProcessing = false;
  type: "webview" | "browser";

  constructor(type: "webview" | "browser") {
    this.type = type;
  }

  protected initializeBaseMembers(): void {
    this.clearConvertResult();
    this.editorPanel = undefined;
    this.isScrollProcessing = false;
  }

  // ---------------------------------------------
  // fLibConvert Related Methods
  // ---------------------------------------------

  protected abstract convertMdToHtml(
    document: vscode.TextDocument,
    context: vscode.ExtensionContext,
    isNeedDataLineNumber?: boolean, // Optional, defaults to true
  ): Promise<fLibConvert.ConvertResult>;

  protected registerConvertResult(res: fLibConvert.ConvertResult): void {
    this.html = res.html;
    this.hast = res.hast;
    this.mdast = res.mdast;
    this.plainText = res.plainText;
  }

  protected clearConvertResult(): void {
    this.html = undefined;
    this.hast = undefined;
    this.mdast = undefined;
    this.plainText = undefined;
  }

  protected isAnyConvertResultEmpty(): boolean {
    return !this.html || !this.hast || !this.mdast || !this.plainText;
  }

  // ---------------------------------------------
  // openPreview Related Methods
  // ---------------------------------------------

  protected getCurrentActiveTextDocument(): vscode.TextDocument | undefined {
    const editorPanel = vscode.window.activeTextEditor;
    const doc = editorPanel?.document;

    if (!editorPanel || !doc || doc.languageId !== "markdown") {
      vscode.window.showErrorMessage(
        vscode.l10n.t("The Markdown file must be active."),
      );
      return;
    }

    this.editorPanel = editorPanel;

    return doc;
  }

  protected scrollEditor(msg: ScrollMessage) {
    if (
      vscode.workspace
        .getConfiguration("fleximark")
        .get<boolean>("shouldSyncScroll") === false
    ) {
      return;
    }

    this.isScrollProcessing = true;

    const position = new vscode.Position(msg.line, 0);
    this.editorPanel?.revealRange(
      new vscode.Range(position, position),
      vscode.TextEditorRevealType.AtTop,
    );
  }

  public abstract openPreview(context: vscode.ExtensionContext): Promise<void>;

  public abstract isPreviewOpened(): boolean;

  // ---------------------------------------------
  // updatePreview Related Methods
  // ---------------------------------------------

  protected abstract makeClientReload(): void;

  public async updatePreview(
    document: vscode.TextDocument,
    context: vscode.ExtensionContext,
    fullReload: boolean,
  ) {
    if (document.languageId !== "markdown" || !this.isPreviewOpened()) return;

    const result = await this.convertMdToHtml(document, context, true);

    if (fullReload || this.isAnyConvertResultEmpty()) {
      this.registerConvertResult(result);
      this.makeClientReload();
    } else {
      const { editScripts, dataLineArray } = fLibUnist.diffHtml.findDiff(
        this.hast as HastRoot,
        result.hast,
      );
      this.registerConvertResult(result);

      if (editScripts.length > 0) {
        this.emitMessageToClient({
          type: "edit",
          editScripts,
          dataLineArray,
        });
      }
    }
  }

  // ---------------------------------------------
  // Public Utility Methods
  // ---------------------------------------------

  public abstract emitMessageToClient(message: object): void;

  public abstract deactivate(): void;
}

export default BaseServer;
