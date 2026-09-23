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

export const ExtensionMode = { Test: 3 } as const;
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
    return this.value;
  }

  toString(): string {
    return this.value;
  }
}

export const __test = {
  workspaceFolder: undefined as unknown,
  textDocuments: [] as unknown[],
  visibleTextEditors: [] as unknown[],
  showTextDocument: (async (document: unknown) => void document) as (
    document: unknown,
  ) => Promise<unknown>,
  reset(): void {
    this.workspaceFolder = undefined;
    this.textDocuments = [];
    this.visibleTextEditors = [];
    this.showTextDocument = async (document: unknown) => void document;
  },
};

const disposable = () => ({ dispose: () => undefined });
export const workspace = {
  get workspaceFolders() {
    return __test.workspaceFolder ? [__test.workspaceFolder] : [];
  },
  get textDocuments() {
    return __test.textDocuments;
  },
  getWorkspaceFolder: () => __test.workspaceFolder,
  getConfiguration: () => ({
    get: (_key: string, fallback?: unknown) => fallback,
    update: async () => undefined,
  }),
  isTrusted: true,
};
export const window = {
  get visibleTextEditors() {
    return __test.visibleTextEditors;
  },
  showTextDocument: (document: unknown) => __test.showTextDocument(document),
  createOutputChannel: () => ({
    append: () => undefined,
    appendLine: () => undefined,
    show: () => undefined,
    dispose: () => undefined,
  }),
  showErrorMessage: async () => undefined,
  showInformationMessage: async () => undefined,
  setStatusBarMessage: disposable,
};
export const languages = { createDiagnosticCollection: disposable };
export const commands = {
  registerCommand: disposable,
  executeCommand: async () => undefined,
};
export const env = { openExternal: async () => true };
export const l10n = { t: (message: string) => message };
