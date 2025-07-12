declare global {
  interface Window {
    renderABC: () => void;
    renderMermaid: () => void;
    webSocketUrl: string;
  }
}

export {};
