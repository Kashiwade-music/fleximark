declare const acquireVsCodeApi: () => {
  postMessage: (message: any) => void;
};

interface ScrollMessage {
  type: "editor-scroll";
  line: number;
}

interface PreviewScrollMessage {
  type: "preview-scroll";
  line: number;
}

const vscode = acquireVsCodeApi();

// スクロールを受け取って反映
window.addEventListener("message", (event: MessageEvent<ScrollMessage>) => {
  const msg = event.data;
  if (msg.type === "editor-scroll") {
    const targetLine = msg.line;

    const elements =
      document.querySelectorAll<HTMLElement>("[data-line-number]");
    const targetElement = Array.from(elements).find((el) => {
      console.log("Checking element:", el.dataset);

      const lineAttr = el.dataset.lineNumber;
      return lineAttr ? parseInt(lineAttr, 10) >= targetLine : false;
    });

    if (targetElement) {
      targetElement.scrollIntoView({ behavior: "smooth", block: "start" });
    }
  }
});

// スクロール時に位置を通知（オプションで双方向）
document.addEventListener(
  "scroll",
  () => {
    const elements =
      document.querySelectorAll<HTMLElement>("[data-line-number]");
    const visibleElement = Array.from(elements).find((el) => {
      const rect = el.getBoundingClientRect();
      return rect.top >= 0;
    });

    if (visibleElement && visibleElement.dataset.lineNumber) {
      const line = parseInt(visibleElement.dataset.lineNumber, 10);
      const message: PreviewScrollMessage = {
        type: "preview-scroll",
        line,
      };
      vscode.postMessage(message);
    }
  },
  true
);
