// ----------------------
// Types & Interfaces
// ----------------------

type OperationType = "insert" | "delete" | "update";

interface HtmlEditScript {
  index: number;
  operation: OperationType;
  newHTMLHash: string;
  newHTML: string;
}

interface DataLine {
  "data-line-number"?: string;
}

interface EditMessage {
  type: "edit";
  dataLineArray: DataLine[];
  editScripts: HtmlEditScript[];
}

interface ReloadMessage {
  type: "reload";
}

interface ScrollMessage {
  type: "editor-scroll";
  line: number;
}

interface PreviewScrollMessage {
  type: "preview-scroll";
  line: number;
}

type ServerMessage = EditMessage | ReloadMessage | ScrollMessage;

// ----------------------
// Constants & State
// ----------------------

const state = {
  isVscodeScrollProcessing: false,
};

// ----------------------
// Event Listeners
// ----------------------

const socket = new WebSocket(window.webSocketUrl);
window.socket = socket; // Expose socket to window for debugging

socket.addEventListener("open", () => {
  console.log("WebSocket connected");
});

socket.addEventListener("close", () => {
  console.log("WebSocket disconnected");
});

socket.addEventListener("error", (err: Event) => {
  console.error("WebSocket error:", err);
});

socket.addEventListener("message", (event: MessageEvent) => {
  const data: ServerMessage = JSON.parse(event.data);

  switch (data.type) {
    case "reload":
      location.reload();
      break;

    case "edit":
      handleEditMessage(data);
      break;

    case "editor-scroll":
      handleEditorScroll(data.line);
      break;

    default:
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      console.warn("Unhandled message type:", (data as any).type);
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

    const line = estimateVisibleLineNumber();
    if (line === undefined) return;

    const message: PreviewScrollMessage = {
      type: "preview-scroll",
      line,
    };

    socket.send(JSON.stringify(message));
  },
  true,
);

// ----------------------
// Message Handlers
// ----------------------

function handleEditMessage(message: EditMessage): void {
  const container = document.querySelector<HTMLDivElement>("div.markdown-body");
  if (!container) {
    console.warn("No .markdown-body element found.");
    return;
  }

  applyEditScripts(container, message.editScripts);
  updateDataLineAttributes(container, message.dataLineArray);
  reRenderSpecialBlocks(message.editScripts);
}

function handleEditorScroll(targetLine: number): void {
  state.isVscodeScrollProcessing = true;

  const elements = Array.from(
    document.querySelectorAll<HTMLElement>("[data-line-number]"),
  );

  let lowerElement: HTMLElement | undefined;
  let upperElement: HTMLElement | undefined;
  let lowerLine: number | undefined;
  let upperLine: number | undefined;

  for (const el of elements) {
    const lineAttr = el.dataset.lineNumber;
    if (!lineAttr) continue;

    const line = parseInt(lineAttr, 10);
    if (isNaN(line)) continue;

    if (line === targetLine) {
      el.scrollIntoView({ behavior: "auto", block: "start" });
      return;
    }

    if (line < targetLine) {
      if (lowerLine === undefined || line > lowerLine) {
        lowerLine = line;
        lowerElement = el;
      }
    } else if (line > targetLine) {
      if (upperLine === undefined || line < upperLine) {
        upperLine = line;
        upperElement = el;
      }
    }
  }

  if (
    lowerElement &&
    upperElement &&
    lowerLine !== undefined &&
    upperLine !== undefined
  ) {
    const ratio = (targetLine - lowerLine) / (upperLine - lowerLine);

    const lowerRect = lowerElement.getBoundingClientRect();
    const upperRect = upperElement.getBoundingClientRect();

    const targetY =
      lowerRect.top + ratio * (upperRect.top - lowerRect.top) + window.scrollY;

    window.scrollTo({ top: targetY, behavior: "auto" });
  } else if (lowerElement) {
    lowerElement.scrollIntoView({ behavior: "auto", block: "start" });
  } else if (upperElement) {
    upperElement.scrollIntoView({ behavior: "auto", block: "start" });
  }
}

// ----------------------
// DOM Utilities
// ----------------------

function estimateVisibleLineNumber(): number | undefined {
  const rawElements = Array.from(
    document.querySelectorAll<HTMLElement>("[data-line-number]"),
  );

  const elementsWithLine = rawElements
    .map((el) => {
      const lineAttr = el.dataset.lineNumber;
      if (lineAttr === undefined) return undefined;

      const line = parseInt(lineAttr, 10);
      if (isNaN(line)) return undefined;

      return { el, line };
    })
    .filter(
      (entry): entry is { el: HTMLElement; line: number } =>
        entry !== undefined,
    );

  if (elementsWithLine.length === 0) return undefined;

  let lower: { el: HTMLElement; line: number } | undefined = undefined;
  let upper: { el: HTMLElement; line: number } | undefined = undefined;

  for (const entry of elementsWithLine) {
    const rect = entry.el.getBoundingClientRect();
    if (rect.top <= 0) {
      if (!lower || entry.line > lower.line) {
        lower = entry;
      }
    } else {
      if (!upper || entry.line < upper.line) {
        upper = entry;
      }
    }
  }

  if (lower && upper) {
    const lowerRect = lower.el.getBoundingClientRect();
    const upperRect = upper.el.getBoundingClientRect();
    const deltaTop = upperRect.top - lowerRect.top;

    if (deltaTop === 0) {
      return lower.line;
    }

    const ratio = (0 - lowerRect.top) / deltaTop;
    return Math.round(lower.line + ratio * (upper.line - lower.line));
  }

  return lower?.line ?? upper?.line;
}

function applyEditScripts(
  container: HTMLElement,
  scripts: HtmlEditScript[],
): void {
  const children = container.children;
  let indexOffset = 0;

  scripts.forEach(({ index, operation, newHTML }) => {
    const temp = document.createElement("div");
    temp.innerHTML = newHTML;
    const newElement = temp.firstElementChild;

    const adjustedIndex = index + indexOffset;

    switch (operation) {
      case "update":
        if (adjustedIndex < children.length && newElement) {
          container.replaceChild(newElement, children[adjustedIndex]);
          newElement.classList.add("fade-highlight");
          setTimeout(() => {
            newElement.classList.remove("fade-highlight");
          }, 1000);
        } else {
          console.warn(`Update failed: index ${adjustedIndex} out of bounds.`);
        }
        break;

      case "insert":
        if (newElement) {
          if (adjustedIndex >= children.length) {
            container.appendChild(newElement);
          } else {
            container.insertBefore(newElement, children[adjustedIndex]);
          }
          indexOffset++;
        }
        break;

      case "delete":
        if (adjustedIndex < children.length) {
          container.removeChild(children[adjustedIndex]);
          indexOffset--;
        } else {
          console.warn(`Delete failed: index ${adjustedIndex} out of bounds.`);
        }
        break;

      default:
        console.warn(`Unknown operation: ${operation}`);
    }
  });
}

function updateDataLineAttributes(
  container: HTMLElement,
  dataLines: DataLine[],
): void {
  dataLines.forEach((dataLine, idx) => {
    const element = container.children[idx];
    if (element && dataLine["data-line-number"]) {
      element.setAttribute("data-line-number", dataLine["data-line-number"]);
    }
  });
}

function reRenderSpecialBlocks(scripts: HtmlEditScript[]): void {
  const needsABC = scripts.some((edit) =>
    edit.newHTML.includes('data-language="abc"'),
  );

  const needsMermaid = scripts.some((edit) =>
    edit.newHTML.includes('data-language="mermaid"'),
  );

  if (needsABC) window.renderABC?.();
  if (needsMermaid) window.renderMermaid?.();
}
