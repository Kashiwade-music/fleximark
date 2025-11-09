import * as Base from "./base.mjs";
import "./shared/abcjsScripts.mjs";
import "./shared/mermaidScripts.mjs";

// ----------------------
// Class
// ----------------------

class BrowserClient extends Base.BaseClient {
  socket: WebSocket;

  constructor() {
    super("browser");
    this.socket = new WebSocket(window.webSocketUrl);

    this.socket.addEventListener("open", () => {
      console.log("WebSocket connected");
    });

    this.socket.addEventListener("close", () => {
      console.log("WebSocket disconnected");
    });

    this.socket.addEventListener("error", (err: Event) => {
      console.error("WebSocket error:", err);
    });

    this.socket.addEventListener("message", (event: MessageEvent) => {
      const data: Base.ServerMessage = JSON.parse(event.data);

      switch (data.type) {
        case "reload":
          location.reload();
          break;

        case "edit":
          this.handleEditMessage(data);
          break;

        case "editor-scroll":
          this.handleEditorScroll(data.line);
          break;

        case "cursor":
          if (data.blockType === "abc") {
            this.handleCursorMessageAbc(data);
          }
          break;

        default:
          // eslint-disable-next-line @typescript-eslint/no-explicit-any
          console.warn("Unhandled message type:", (data as any).type);
      }
    });
  }

  public override emitMessageToServer(message: object): void {
    this.socket.send(JSON.stringify(message));
  }
}

// ----------------------
// Main Execution
// ----------------------

const browserClient = new BrowserClient();

// Handle messages from VSCode (Editor → Preview)

// Notify VSCode on scroll (Preview → Editor)
document.addEventListener(
  "wheel",
  () => {
    browserClient.markUserScroll();
  },
  { passive: true },
);
document.addEventListener(
  "touchmove",
  () => {
    browserClient.markUserScroll();
  },
  { passive: true },
);
document.addEventListener("keydown", (e) => {
  if (
    ["ArrowUp", "ArrowDown", "PageUp", "PageDown", "Space"].includes(e.code)
  ) {
    browserClient.markUserScroll();
  }
});

document.addEventListener(
  "scroll",
  () => {
    browserClient.scrollEventListener();
  },
  true,
);
