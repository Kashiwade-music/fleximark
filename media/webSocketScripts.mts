interface HtmlEditScript {
  index: number;
  operation: "insert" | "delete" | "update";
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

const socket = new WebSocket(window.webSocketUrl);

socket.addEventListener("message", (event: MessageEvent) => {
  const data: ServerMessage = JSON.parse(event.data);

  // debug log
  console.log("Received message:", JSON.stringify(data, null, 2));

  if (data.type === "reload") {
    location.reload();
    return;
  }

  if (data.type === "edit") {
    const container =
      document.querySelector<HTMLDivElement>("div.markdown-body");
    if (!container) {
      console.warn("No .markdown-body element found.");
      return;
    }

    const children = container.children;

    let indexOffset = 0;

    data.editScripts.forEach((edit: HtmlEditScript) => {
      const { index, operation, newHTML } = edit;

      const temp = document.createElement("div");
      temp.innerHTML = newHTML;
      const newElement = temp.firstElementChild;

      if (operation === "update") {
        if (index + indexOffset < children.length && newElement) {
          container.replaceChild(newElement, children[index + indexOffset]);
        } else {
          console.warn(
            `Update failed: index ${index + indexOffset} out of bounds.`
          );
        }
      } else if (operation === "insert") {
        if (newElement) {
          if (index + indexOffset >= children.length) {
            container.appendChild(newElement);
          } else {
            container.insertBefore(newElement, children[index + indexOffset]);
          }
          indexOffset++;
        }
      } else if (operation === "delete") {
        if (index + indexOffset < children.length) {
          container.removeChild(children[index + indexOffset]);
          indexOffset--;
        } else {
          console.warn(
            `Delete failed: index ${index + indexOffset} out of bounds.`
          );
        }
      } else {
        console.warn(`Unknown operation: ${operation}`);
      }
    });

    // Update the data-line-number attributes
    data.dataLineArray.forEach((dataLine, index) => {
      const element = container.children[index];
      if (element && dataLine["data-line-number"]) {
        element.setAttribute("data-line-number", dataLine["data-line-number"]);
      }
    });

    // if data.htmlEditScript contains ABC or Mermaid, re-render them
    const isABC = data.editScripts.some((edit) =>
      edit.newHTML.includes('data-language="abc"')
    );
    const isMermaid = data.editScripts.some((edit) =>
      edit.newHTML.includes('data-language="mermaid"')
    );

    if (isABC) {
      window.renderABC();
    }
    if (isMermaid) {
      window.renderMermaid();
    }

    return;
  }

  if (data.type === "editor-scroll") {
    if (!globalState.editorScrollFromVscode.enabled) return;

    const targetLine = data.line;

    const elements =
      document.querySelectorAll<HTMLElement>("[data-line-number]");
    const targetElement = Array.from(elements).find((el) => {
      const lineAttr = el.dataset.lineNumber;
      return lineAttr ? parseInt(lineAttr, 10) >= targetLine : false;
    });

    if (targetElement) {
      targetElement.scrollIntoView({ behavior: "smooth", block: "start" });
    }
    return;
  }
});

socket.addEventListener("open", () => {
  console.log("WebSocket connected");
});

socket.addEventListener("close", () => {
  console.log("WebSocket disconnected");
});

socket.addEventListener("error", (err: Event) => {
  console.error("WebSocket error:", err);
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
      console.log("Sending scroll message:", message);

      socket.send(JSON.stringify(message));
    }
  },
  true
);
