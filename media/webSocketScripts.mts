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

type ServerMessage = EditMessage | ReloadMessage;

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
