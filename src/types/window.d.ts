declare global {
  interface Window {
    renderABC: () => void;
    renderMermaid: () => void;
    webSocketUrl: string;
    socket: WebSocket;
  }
}

export {};
