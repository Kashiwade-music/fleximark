import { fileURLToPath } from "node:url";

export class Position {
  constructor(
    readonly line: number,
    readonly character: number,
  ) {}
}

export class Range {
  readonly start: Position;
  readonly end: Position;

  constructor(
    startLine: number,
    startCharacter: number,
    endLine: number,
    endCharacter: number,
  ) {
    this.start = new Position(startLine, startCharacter);
    this.end = new Position(endLine, endCharacter);
  }
}

export class Selection {
  constructor(
    readonly anchor: Position,
    readonly active: Position,
  ) {}

  get start(): Position {
    return this.anchor;
  }

  get end(): Position {
    return this.active;
  }
}

export const ExtensionMode = { Production: 1, Test: 3 } as const;
export const TextEditorRevealType = {
  InCenterIfOutsideViewport: 0,
  AtTop: 1,
} as const;
export const ConfigurationTarget = { WorkspaceFolder: 3 } as const;
export const ViewColumn = { Active: 1, Beside: 2 } as const;
export const DiagnosticSeverity = { Error: 0, Warning: 1 } as const;

export class Diagnostic {
  source?: string;
  code?: unknown;
  data?: unknown;

  constructor(
    readonly range: Range,
    readonly message: string,
    readonly severity: number,
  ) {}
}

export class Uri {
  constructor(readonly value: string) {}

  static parse(value: string): Uri {
    return new Uri(value);
  }

  static joinPath(base: Uri, ...parts: string[]): Uri {
    return new Uri([base.value, ...parts].join("/"));
  }

  get fsPath(): string {
    return this.value.startsWith("file:")
      ? fileURLToPath(this.value)
      : this.value;
  }

  toString(): string {
    return this.value;
  }
}

export class RuntimePanel {
  title = "";
  revealCount = 0;
  disposed = false;
  readonly messages: unknown[] = [];
  readonly #receivers = new Set<(message: unknown) => unknown>();
  readonly #disposers = new Set<() => unknown>();
  readonly webview = {
    html: "",
    cspSource: "https://fake-webview.invalid",
    asWebviewUri: (uri: Uri) => uri,
    postMessage: async (message: unknown) => {
      if (this.disposed) return false;
      this.messages.push(message);
      return true;
    },
    onDidReceiveMessage: (listener: (message: unknown) => unknown) => {
      this.#receivers.add(listener);
      return { dispose: () => this.#receivers.delete(listener) };
    },
  };

  get html(): string {
    return this.webview.html;
  }

  onDidDispose(listener: () => unknown) {
    this.#disposers.add(listener);
    return { dispose: () => this.#disposers.delete(listener) };
  }

  reveal(): void {
    this.revealCount += 1;
  }

  ready(): void {
    this.receive({ type: "ready" });
  }

  sendNavigation(event: Record<string, unknown>): void {
    this.receive(event);
  }

  receive(message: Record<string, unknown>): void {
    for (const listener of this.#receivers) listener(message);
  }

  dispose(): void {
    if (this.disposed) return;
    this.disposed = true;
    for (const listener of this.#disposers) listener();
    this.#receivers.clear();
    this.#disposers.clear();
  }
}

export const __test = {
  workspaceFolder: undefined as unknown,
  workspaceFolders: undefined as
    { uri: Uri; name: string; index: number }[] | undefined,
  activeTextEditor: undefined as unknown,
  daemonPath: undefined as string | undefined,
  logLevel: "info",
  outputLines: [] as string[],
  panels: [] as RuntimePanel[],
  externalUrls: [] as string[],
  errors: [] as unknown[],
  textDocuments: [] as unknown[],
  visibleTextEditors: [] as unknown[],
  showTextDocument: (async (document: unknown) => void document) as (
    document: unknown,
  ) => Promise<unknown>,
  reset(): void {
    this.workspaceFolder = undefined;
    this.workspaceFolders = undefined;
    this.activeTextEditor = undefined;
    this.daemonPath = undefined;
    this.logLevel = "info";
    this.outputLines = [];
    for (const panel of this.panels) panel.dispose();
    this.panels = [];
    this.externalUrls = [];
    this.errors = [];
    this.textDocuments = [];
    this.visibleTextEditors = [];
    this.showTextDocument = async (document: unknown) => void document;
  },
};

const disposable = () => ({ dispose: () => undefined });
export const workspace = {
  get workspaceFolders() {
    return (
      __test.workspaceFolders ??
      (__test.workspaceFolder ? [__test.workspaceFolder] : [])
    );
  },
  get textDocuments() {
    return __test.textDocuments;
  },
  getWorkspaceFolder: (uri?: Uri) => {
    if (__test.workspaceFolders === undefined) return __test.workspaceFolder;
    const value = uri?.toString();
    return __test.workspaceFolders.find(
      (folder) =>
        value === folder.uri.toString() ||
        value?.startsWith(folder.uri.toString() + "/"),
    );
  },
  getConfiguration: () => ({
    get: (key: string, fallback?: unknown) =>
      key === "daemonPath"
        ? (__test.daemonPath ?? fallback)
        : key === "logLevel"
          ? __test.logLevel
          : fallback,
    update: async () => undefined,
  }),
  isTrusted: true,
};
export const window = {
  get activeTextEditor() {
    return __test.activeTextEditor;
  },
  get visibleTextEditors() {
    return __test.visibleTextEditors;
  },
  showTextDocument: (document: unknown) => __test.showTextDocument(document),
  createOutputChannel: () => ({
    append: () => undefined,
    appendLine: (value: string) => {
      __test.outputLines.push(value);
    },
    show: () => undefined,
    dispose: () => undefined,
  }),
  showErrorMessage: async (message: unknown) => {
    __test.errors.push(message);
    return undefined;
  },
  showInformationMessage: async () => undefined,
  createWebviewPanel: (_type: string, title: string) => {
    const panel = new RuntimePanel();
    panel.title = title;
    __test.panels.push(panel);
    return panel;
  },
  setStatusBarMessage: disposable,
};
export const languages = { createDiagnosticCollection: disposable };
export const commands = {
  registerCommand: disposable,
  executeCommand: async () => undefined,
};
export const env = {
  openExternal: async (uri: Uri) => {
    __test.externalUrls.push(uri.toString());
    return true;
  },
};
export const l10n = { t: (message: string) => message };
