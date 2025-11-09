import * as Base from "./base.mjs";
import "./shared/abcjsScripts.mjs";
import "./shared/mermaidScripts.mjs";
import "./shared/youtubePlaceholderScripts.mjs";

// ----------------------
// VSCode API
// ----------------------

declare const acquireVsCodeApi: () => {
  postMessage: (message: unknown) => void;
};

const vscode = acquireVsCodeApi();

// ----------------------
// Class
// ----------------------

class WebviewClient extends Base.BaseClient {
  constructor() {
    super("webview");
  }

  public override emitMessageToServer(message: object): void {
    vscode.postMessage(JSON.stringify(message));
  }
}

// ----------------------
// Main Execution
// ----------------------

const webviewClient = new WebviewClient();

// Handle messages from VSCode (Editor → Preview)
window.addEventListener(
  "message",
  (event: MessageEvent<Base.ServerMessage>) => {
    const data = event.data;

    switch (data.type) {
      case "edit":
        webviewClient.handleEditMessage(data);
        break;

      case "editor-scroll":
        webviewClient.handleEditorScroll(data.line);
        break;

      case "cursor":
        if (data.blockType === "abc") {
          webviewClient.handleCursorMessageAbc(data);
        }
        break;

      default:
        // eslint-disable-next-line @typescript-eslint/no-explicit-any
        console.warn("Unhandled message type:", (data as any).type);
    }
  },
);

// Notify VSCode on scroll (Preview → Editor)
document.addEventListener(
  "wheel",
  () => {
    webviewClient.markUserScroll();
  },
  { passive: true },
);
document.addEventListener(
  "touchmove",
  () => {
    webviewClient.markUserScroll();
  },
  { passive: true },
);
document.addEventListener("keydown", (e) => {
  if (
    ["ArrowUp", "ArrowDown", "PageUp", "PageDown", "Space"].includes(e.code)
  ) {
    webviewClient.markUserScroll();
  }
});

document.addEventListener(
  "scroll",
  () => {
    webviewClient.scrollEventListener();
  },
  true,
);
