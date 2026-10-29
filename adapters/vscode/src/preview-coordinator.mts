export function embeddedPreviewShell(
  nonce: string,
  messageToken: string,
  cspSource: string,
  scriptUri: string,
  css: string,
): string {
  return `<!doctype html><html><head><meta charset="utf-8"><meta http-equiv="Content-Security-Policy" content="default-src 'none'; base-uri 'none'; form-action 'none'; script-src 'nonce-${nonce}' ${cspSource}; style-src ${cspSource} 'unsafe-inline'; font-src data:; img-src ${cspSource} https://i.ytimg.com data: blob:; media-src ${cspSource} blob:; frame-src https://www.youtube-nocookie.com; object-src 'none';"><meta name="referrer" content="strict-origin-when-cross-origin"><meta name="viewport" content="width=device-width,initial-scale=1"><meta name="fleximark-message-token" content="${messageToken}"><style>${css}</style></head><body><main id="preview" class="markdown-body"></main><script nonce="${nonce}" src="${scriptUri}"></script></body></html>`;
}

export interface PreviewEchoState {
  selection?: { uri: string; value: string };
  viewport?: { uri: string };
}
