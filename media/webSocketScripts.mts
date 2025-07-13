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

  console.log("Received message:", JSON.stringify(data, null, 2));

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

    const firstVisibleElement = getFirstVisibleLineElement();
    if (!firstVisibleElement?.dataset.lineNumber) return;

    const line = parseInt(firstVisibleElement.dataset.lineNumber, 10);

    const message: PreviewScrollMessage = {
      type: "preview-scroll",
      line,
    };

    socket.send(JSON.stringify(message));
  },
  true
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

function applyEditScripts(
  container: HTMLElement,
  scripts: HtmlEditScript[]
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
  dataLines: DataLine[]
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
    edit.newHTML.includes('data-language="abc"')
  );

  const needsMermaid = scripts.some((edit) =>
    edit.newHTML.includes('data-language="mermaid"')
  );

  if (needsABC) window.renderABC?.();
  if (needsMermaid) window.renderMermaid?.();
}
