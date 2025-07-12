declare const acquireVsCodeApi: () => {
  postMessage: (message: any) => void;
};

interface ScrollState {
  enabled: boolean;
  timer: ReturnType<typeof setTimeout> | null;
}

const timerMS = 600;

const globalState = {
  editorScrollFromVscode: {
    enabled: true,
    timer: null as ReturnType<typeof setTimeout> | null,
  } as ScrollState,
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
    if (!globalState.editorScrollFromVscode.enabled) return;

    const targetLine = msg.line;

    const elements =
      document.querySelectorAll<HTMLElement>("[data-line-number]");
    const targetElement = Array.from(elements).find((el) => {
      const lineAttr = el.dataset.lineNumber;
      return lineAttr ? parseInt(lineAttr, 10) >= targetLine : false;
    });

    if (targetElement) {
      targetElement.scrollIntoView({ behavior: "smooth", block: "start" });
    }
  }
});

// スクロール時に位置を通知
document.addEventListener(
  "scroll",
  () => {
    if (globalState.editorScrollFromVscode.timer) {
      clearTimeout(globalState.editorScrollFromVscode.timer);
    }
    globalState.editorScrollFromVscode.enabled = false;
    globalState.editorScrollFromVscode.timer = setTimeout(() => {
      globalState.editorScrollFromVscode.enabled = true;
    }, timerMS);

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
