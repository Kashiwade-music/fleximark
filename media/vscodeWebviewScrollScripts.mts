// ----------------------
// VSCode API
// ----------------------

declare const acquireVsCodeApi: () => {
  postMessage: (message: unknown) => void;
};

const vscode = acquireVsCodeApi();

// ----------------------
// Types & Interfaces
// ----------------------

interface ScrollMessage {
  type: "editor-scroll";
  line: number;
}

interface PreviewScrollMessage {
  type: "preview-scroll";
  line: number;
}

// ----------------------
// Constants & State
// ----------------------

const state = {
  isVscodeScrollProcessing: false,
};

// ----------------------
// Event Listeners
// ----------------------

// Handle messages from VSCode (Editor → Preview)
window.addEventListener("message", (event: MessageEvent<ScrollMessage>) => {
  const message = event.data;

  if (message.type === "editor-scroll") {
    handleEditorScroll(message.line);
  }
});

// Notify VSCode on scroll (Preview → Editor)
document.addEventListener(
  "scroll",
  () => {
    if (state.isVscodeScrollProcessing) {
      state.isVscodeScrollProcessing = false;
      return;
    }

    const firstVisibleElement = getFirstVisibleLineElement();
    if (!firstVisibleElement?.dataset.lineNumber) return;

    const line = parseInt(firstVisibleElement.dataset.lineNumber, 10);

    const message: PreviewScrollMessage = {
      type: "preview-scroll",
      line,
    };

    vscode.postMessage(message);
  },
  true
);

// ----------------------
// Message Handlers
// ----------------------

function handleEditorScroll(targetLine: number): void {
  state.isVscodeScrollProcessing = true;

  const targetElement = findLineElementAtOrAfter(targetLine);

  if (targetElement) {
    targetElement.scrollIntoView({ behavior: "auto", block: "start" });
  }
}

// ----------------------
// DOM Utilities
// ----------------------

function findLineElementAtOrAfter(line: number): HTMLElement | undefined {
  const elements = document.querySelectorAll<HTMLElement>("[data-line-number]");

  return Array.from(elements).find((el) => {
    const lineAttr = el.dataset.lineNumber;
    return lineAttr ? parseInt(lineAttr, 10) >= line : false;
  });
}

function getFirstVisibleLineElement(): HTMLElement | undefined {
  const elements = document.querySelectorAll<HTMLElement>("[data-line-number]");

  return Array.from(elements).find((el) => {
    const rect = el.getBoundingClientRect();
    return rect.top >= 0;
  });
}
