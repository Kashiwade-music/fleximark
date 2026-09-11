import { type ChildProcessWithoutNullStreams, spawn } from "node:child_process";
import { createHash, randomBytes } from "node:crypto";
import { readFile } from "node:fs/promises";
import * as path from "node:path";
import * as vscode from "vscode";

import type {
  RenderPublication,
  SourcePosition,
} from "../../../web/preview-client/index.mjs";
import {
  type AttachDocumentResult,
  type CommandResult,
  type CreatePreviewResult,
  type ExecuteCommandParams,
  type GetNoteOptionsParams,
  type GetNoteOptionsResult,
  type InitializeResult,
  type PreviewClientEvent,
  type PreviewEvent,
  type PreviewTarget,
  type RequestFullTextParams,
  type SourceNavigationEvent,
  protocolVersion,
} from "./protocol.mjs";
import {
  JsonRpcConnection,
  type JsonRpcRequest,
  JsonRpcResponseError,
} from "./rpc.mjs";

interface DocumentState {
  sessionId?: string;
  version: number;
  syncing?: Promise<void>;
  checkpoint?: NodeJS.Timeout;
}

interface PreviewState {
  documentUri: string;
  previewSessionId: string;
  target: PreviewTarget;
  url?: string;
  initialPublication: RenderPublication;
  renderRevision: number;
  panel?: vscode.WebviewPanel;
}

interface WorkspaceRuntime {
  workspace: vscode.WorkspaceFolder;
  documents: Map<string, DocumentState>;
  previews: Map<string, PreviewState>;
  removed: boolean;
}

interface DaemonRuntime {
  process?: ChildProcessWithoutNullStreams;
  rpc?: JsonRpcConnection;
  daemonInstanceId?: string;
  startPromise?: Promise<void>;
  restartTimer?: NodeJS.Timeout;
  restartCount: number;
  lastStartedAt: number;
  connectionGeneration: number;
  workspaceRevision: number;
  appliedWorkspaceRevision: number;
  restarting: boolean;
  recoverySequence: number;
  recoveryId?: string;
  recoveryStatus?: vscode.Disposable;
}

interface ReleaseArtifact {
  platform: string;
  arch: string;
  path: string;
  sha256: string;
}

interface ReleaseManifest {
  schemaVersion: number;
  artifacts: ReleaseArtifact[];
}

export interface AdapterRecoveryState {
  connectionGeneration: number;
  daemonInstanceId?: string;
  documentSessions: Record<string, string | undefined>;
  previewSessions: Record<string, string>;
}

export function sourcePositionToCharacter(
  line: string,
  position: SourcePosition,
): number {
  if (position.encoding === "utf16") return position.character;
  let sourceUnits = 0;
  let utf16Units = 0;
  for (const character of line) {
    const next =
      sourceUnits +
      (position.encoding === "utf8" ? Buffer.byteLength(character, "utf8") : 1);
    if (next > position.character) break;
    sourceUnits = next;
    utf16Units += character.length;
  }
  return utf16Units;
}

export async function executeCreateNote(
  params: ExecuteCommandParams,
  getOptions: (params: GetNoteOptionsParams) => Promise<GetNoteOptionsResult>,
  pick: (
    items: readonly string[],
    options: { placeHolder: string },
  ) => Thenable<string | undefined>,
  execute: (params: ExecuteCommandParams) => Promise<CommandResult>,
): Promise<CommandResult | undefined> {
  if (!params.workspaceUri) return execute(params);
  const options = await getOptions({
    daemonInstanceId: params.daemonInstanceId,
    workspaceUri: params.workspaceUri,
  });
  const noteCategory = options.categories.length
    ? await pick(options.categories, {
        placeHolder: vscode.l10n.t("Select a note category"),
      })
    : undefined;
  if (options.categories.length && noteCategory === undefined) return;
  const noteTemplate = options.templates.length
    ? await pick(options.templates, {
        placeHolder: vscode.l10n.t("Select a note template"),
      })
    : undefined;
  if (options.templates.length && noteTemplate === undefined) return;
  return execute({ ...params, noteCategory, noteTemplate });
}

export async function openCommandResult(
  params: ExecuteCommandParams,
  result: CommandResult,
  openUri: (uri: string) => Promise<void>,
  execute: (params: ExecuteCommandParams) => Promise<CommandResult>,
): Promise<void> {
  if (!result.openUri) return;
  await openUri(result.openUri);
  if (params.command === "exportHtml")
    await execute({ ...params, command: "acknowledgeExport" });
}

export async function selectWorkspaceUri(
  activeWorkspaceUri: string | undefined,
  workspaces: readonly { label: string; uri: string }[],
  pick: (
    items: readonly { label: string; uri: string }[],
    options: { placeHolder: string },
  ) => Thenable<{ label: string; uri: string } | undefined>,
): Promise<string | undefined> {
  if (
    activeWorkspaceUri &&
    workspaces.some((workspace) => workspace.uri === activeWorkspaceUri)
  )
    return activeWorkspaceUri;
  if (workspaces.length === 1) return workspaces[0].uri;
  return (
    await pick(workspaces, {
      placeHolder: vscode.l10n.t("Select a FlexiMark workspace"),
    })
  )?.uri;
}

export class FlexiMarkAdapter implements vscode.Disposable {
  readonly #context: vscode.ExtensionContext;
  readonly #output = vscode.window.createOutputChannel("FlexiMark");
  readonly #diagnostics =
    vscode.languages.createDiagnosticCollection("fleximark");
  readonly #runtimes = new Map<string, WorkspaceRuntime>();
  readonly #daemon: DaemonRuntime = {
    restartCount: 0,
    lastStartedAt: 0,
    connectionGeneration: 0,
    workspaceRevision: 0,
    appliedWorkspaceRevision: -1,
    restarting: false,
    recoverySequence: 0,
  };
  #stopping = false;
  #selectionEcho?: { uri: string; value: string };
  #viewportEcho?: { uri: string };

  constructor(context: vscode.ExtensionContext) {
    this.#context = context;
  }

  async start(
    workspace?: vscode.WorkspaceFolder,
  ): Promise<WorkspaceRuntime | undefined> {
    if (!workspace) return;
    const key = workspace.uri.toString();
    let runtime = this.#runtimes.get(key);
    if (!runtime) {
      runtime = {
        workspace,
        documents: new Map(),
        previews: new Map(),
        removed: false,
      };
      this.#runtimes.set(key, runtime);
      this.#daemon.workspaceRevision += 1;
    }
    await this.#ensureDaemon();
    return runtime;
  }

  async activateDocument(document?: vscode.TextDocument): Promise<void> {
    const workspace = document
      ? vscode.workspace.getWorkspaceFolder(document.uri)
      : undefined;
    if (!document || !workspace) return;
    await this.start(workspace);
    if (document?.languageId === "markdown") await this.syncDocument(document);
  }

  async requestLanguageFeature<T>(
    method: string,
    document: vscode.TextDocument,
    params: object = {},
  ): Promise<T | undefined> {
    await this.activateDocument(document);
    if (!this.#daemon.rpc) throw new Error("FlexiMark daemon is unavailable");
    const rpc = this.#daemon.rpc;
    try {
      return await rpc.request<T>(method, {
        textDocument: { uri: document.uri.toString() },
        ...params,
      });
    } catch (error) {
      if (
        rpc.closed ||
        (error instanceof JsonRpcResponseError &&
          error.code === -32602 &&
          error.message.startsWith("document is not open"))
      )
        return undefined;
      throw error;
    }
  }

  async openPreview(target: PreviewTarget): Promise<void> {
    const document = vscode.window.activeTextEditor?.document;
    if (!document || document.languageId !== "markdown") {
      void vscode.window.showInformationMessage(
        vscode.l10n.t("Open a Markdown document first."),
      );
      return;
    }
    const workspace = vscode.workspace.getWorkspaceFolder(document.uri);
    const runtime = await this.start(workspace);
    if (!runtime) return;
    await this.syncDocument(document);
    const result = await this.#requestPreview(runtime, document, target);
    if (!result) return;
    if (target === "externalBrowser") {
      if (!result.url)
        throw new Error("daemon omitted the external preview URL");
      runtime.previews.set(result.previewSessionId, {
        documentUri: document.uri.toString(),
        previewSessionId: result.previewSessionId,
        target,
        url: result.url,
        initialPublication: result.initialPublication,
        renderRevision: result.initialPublication.resultRenderRevision,
      });
      await vscode.env.openExternal(vscode.Uri.parse(result.url));
      return;
    }

    const panel = vscode.window.createWebviewPanel(
      "fleximark.preview",
      `FlexiMark: ${path.basename(document.fileName)}`,
      vscode.workspace
        .getConfiguration("fleximark")
        .get<"active" | "beside">("previewColumn", "beside") === "active"
        ? vscode.ViewColumn.Active
        : vscode.ViewColumn.Beside,
      { enableScripts: true, retainContextWhenHidden: true },
    );
    const htmlUri = vscode.Uri.joinPath(
      this.#context.extensionUri,
      "dist",
      "web",
      "preview-client",
      "vscode-host.js",
    );
    const scriptUri = panel.webview.asWebviewUri(htmlUri);
    const nonce = randomBytes(16).toString("base64");
    const shell = `<!doctype html><html><head><meta charset="utf-8"><meta http-equiv="Content-Security-Policy" content="default-src 'none'; script-src 'nonce-${nonce}' ${panel.webview.cspSource}; style-src ${panel.webview.cspSource} 'unsafe-inline'; img-src ${panel.webview.cspSource} data: blob:; media-src ${panel.webview.cspSource} blob:; frame-src https://www.youtube-nocookie.com; object-src 'none';"><meta name="viewport" content="width=device-width,initial-scale=1"><style>html,body,#preview{min-height:100%;margin:0}[data-fleximark-selected=true]{outline:2px solid var(--vscode-focusBorder);outline-offset:2px}.fleximark-token-keyword{color:var(--vscode-symbolIcon-keywordForeground)}.fleximark-token-string{color:var(--vscode-symbolIcon-stringForeground)}.fleximark-token-number{color:var(--vscode-symbolIcon-numberForeground)}.fleximark-token-comment{color:var(--vscode-descriptionForeground)}</style></head><body><main id="preview"></main><script nonce="${nonce}" src="${scriptUri}"></script></body></html>`;
    const preview: PreviewState = {
      documentUri: document.uri.toString(),
      previewSessionId: result.previewSessionId,
      target,
      url: result.url,
      initialPublication: result.initialPublication,
      renderRevision: result.initialPublication.resultRenderRevision,
      panel,
    };
    runtime.previews.set(result.previewSessionId, preview);
    panel.webview.onDidReceiveMessage(
      (event: {
        type?: string;
        nodeId?: unknown;
        previewSessionId?: unknown;
        renderRevision?: unknown;
      }) => {
        if (event.type === "ready") {
          void panel.webview.postMessage({
            type: "initializePreview",
            publication: preview.initialPublication,
          });
          return;
        }
        if (event.type === "requestSnapshot") {
          void this.#reloadPreview(runtime, preview);
          return;
        }
        if (
          (event.type === "selectNode" || event.type === "revealNode") &&
          typeof event.nodeId === "string" &&
          typeof event.previewSessionId === "string" &&
          typeof event.renderRevision === "number"
        ) {
          this.#daemon.rpc?.notify("fleximark/previewEvent", {
            daemonInstanceId: this.#daemon.daemonInstanceId,
            previewSessionId: event.previewSessionId,
            renderRevision: event.renderRevision,
            event: event as PreviewClientEvent,
          });
        }
      },
    );
    panel.onDidDispose(() => void this.#disposePreview(runtime, preview));
    panel.webview.html = shell;
  }

  async syncDocument(document: vscode.TextDocument): Promise<void> {
    if (document.languageId !== "markdown") return;
    const workspace = vscode.workspace.getWorkspaceFolder(document.uri);
    const runtime = await this.start(workspace);
    if (!runtime || !this.#daemon.rpc) return;
    await this.#syncDocument(runtime, document);
  }

  async #syncDocument(
    runtime: WorkspaceRuntime,
    document: vscode.TextDocument,
  ): Promise<void> {
    const uri = document.uri.toString();
    const existing = runtime.documents.get(uri);
    if (existing?.sessionId) return;
    if (existing?.syncing) return existing.syncing;
    const state: DocumentState = existing ?? { version: document.version };
    state.syncing = (async () => {
      const version = document.version;
      const text = document.getText();
      this.#daemon.rpc?.notify("textDocument/didOpen", {
        textDocument: { uri, languageId: document.languageId, version, text },
      });
      const attached = await this.#daemon.rpc?.request<AttachDocumentResult>(
        "fleximark/attachDocument",
        {
          daemonInstanceId: this.#daemon.daemonInstanceId,
          uri,
          expectedDocumentVersion: version,
          contentHash: createHash("sha256").update(text).digest("hex"),
        },
      );
      if (!attached) return;
      state.sessionId = attached.documentSessionId;
      state.version = version;
      if (document.version !== version) this.changeDocument(document);
    })().finally(() => (state.syncing = undefined));
    runtime.documents.set(uri, state);
    return state.syncing;
  }

  changeDocument(document: vscode.TextDocument): void {
    if (document.languageId !== "markdown") return;
    const runtime = this.#runtimeForDocument(document);
    const state = runtime?.documents.get(document.uri.toString());
    if (!state?.sessionId || !runtime || !this.#daemon.rpc) return;
    state.version = document.version;
    this.#daemon.rpc?.notify("textDocument/didChange", {
      textDocument: { uri: document.uri.toString(), version: document.version },
      contentChanges: [{ text: document.getText() }],
    });
    if (state.checkpoint) clearTimeout(state.checkpoint);
    state.checkpoint = setTimeout(() => {
      void this.#checkpoint(runtime, document, state).catch((error: unknown) =>
        this.#report(error),
      );
    }, 150);
  }

  closeDocument(document: vscode.TextDocument): void {
    const uri = document.uri.toString();
    const runtime = this.#runtimeForDocument(document);
    if (!runtime) return;
    const state = runtime.documents.get(uri);
    if (state?.checkpoint) clearTimeout(state.checkpoint);
    this.#daemon.rpc?.notify("textDocument/didClose", {
      textDocument: { uri },
    });
    for (const preview of [...runtime.previews.values()]) {
      if (preview.documentUri === uri)
        void this.#disposePreview(runtime, preview);
    }
    runtime.documents.delete(uri);
  }

  selectionChanged(event: vscode.TextEditorSelectionChangeEvent): void {
    const uri = event.textEditor.document.uri.toString();
    const selectionValue = event.selections
      .map(
        ({ anchor, active }) =>
          `${anchor.line}:${anchor.character}-${active.line}:${active.character}`,
      )
      .join(",");
    if (
      this.#selectionEcho?.uri === uri &&
      this.#selectionEcho.value === selectionValue
    ) {
      this.#selectionEcho = undefined;
      return;
    }
    const runtime = this.#runtimeForDocument(event.textEditor.document);
    const state = runtime?.documents.get(uri);
    if (!state?.sessionId || !this.#daemon.daemonInstanceId) return;
    this.#daemon.rpc?.notify("fleximark/setSelection", {
      daemonInstanceId: this.#daemon.daemonInstanceId,
      documentSessionId: state.sessionId,
      expectedDocumentVersion: event.textEditor.document.version,
      selections: event.selections.map((selection) => ({
        anchor: selection.anchor,
        active: selection.active,
      })),
    });
  }

  viewportChanged(event: vscode.TextEditorVisibleRangesChangeEvent): void {
    const uri = event.textEditor.document.uri.toString();
    if (this.#viewportEcho?.uri === uri) {
      this.#viewportEcho = undefined;
      return;
    }
    const runtime = this.#runtimeForDocument(event.textEditor.document);
    const state = runtime?.documents.get(uri);
    if (!state?.sessionId || !this.#daemon.daemonInstanceId) return;
    this.#daemon.rpc?.notify("fleximark/setViewport", {
      daemonInstanceId: this.#daemon.daemonInstanceId,
      documentSessionId: state.sessionId,
      expectedDocumentVersion: event.textEditor.document.version,
      ranges: event.visibleRanges.map((range) => ({
        start: { line: range.start.line, character: range.start.character },
        end: { line: range.end.line, character: range.end.character },
      })),
    });
  }

  async execute(command: string): Promise<void> {
    const document = vscode.window.activeTextEditor?.document;
    const folders = vscode.workspace.workspaceFolders ?? [];
    const activeWorkspace = document
      ? vscode.workspace.getWorkspaceFolder(document.uri)
      : undefined;
    const workspaceUri = await selectWorkspaceUri(
      activeWorkspace?.uri.toString(),
      folders.map((folder) => ({
        label: folder.name,
        uri: folder.uri.toString(),
      })),
      vscode.window.showQuickPick,
    );
    if (!workspaceUri) return;
    const workspace = folders.find(
      (folder) => folder.uri.toString() === workspaceUri,
    );
    const runtime = await this.start(workspace);
    if (!runtime) return;
    if (document?.languageId === "markdown") await this.syncDocument(document);
    const state = document && runtime.documents.get(document.uri.toString());
    const rpc = this.#daemon.rpc;
    if (!this.#daemon.daemonInstanceId || !rpc) return;
    const params: ExecuteCommandParams = {
      daemonInstanceId: this.#daemon.daemonInstanceId,
      command,
      documentSessionId: state?.sessionId,
      expectedDocumentVersion: document?.version,
      workspaceUri,
    };
    const result =
      command === "createNote"
        ? await executeCreateNote(
            params,
            (optionParams) =>
              rpc.request<GetNoteOptionsResult>(
                "fleximark/getNoteOptions",
                optionParams,
              ),
            vscode.window.showQuickPick,
            (commandParams) =>
              rpc.request<CommandResult>(
                "fleximark/executeCommand",
                commandParams,
              ),
          )
        : await rpc.request<CommandResult>("fleximark/executeCommand", params);
    if (!result) return;
    await openCommandResult(
      params,
      result,
      async (uri) => {
        await vscode.window.showTextDocument(vscode.Uri.parse(uri));
      },
      (acknowledgement) =>
        rpc.request<CommandResult>("fleximark/executeCommand", acknowledgement),
    );
    if (result?.message) {
      const show =
        result.message.level === "error"
          ? vscode.window.showErrorMessage
          : result.message.level === "warning"
            ? vscode.window.showWarningMessage
            : vscode.window.showInformationMessage;
      await show(result.message.text);
    }
  }

  async forceReload(): Promise<void> {
    await Promise.all(
      [...this.#runtimes.values()].flatMap((runtime) =>
        [...runtime.previews.values()].map((preview) =>
          this.#reloadPreview(runtime, preview),
        ),
      ),
    );
  }

  async reconfigureWorkspace(workspace: vscode.WorkspaceFolder): Promise<void> {
    const runtime = await this.start(workspace);
    const rpc = this.#daemon.rpc;
    const daemonInstanceId = this.#daemon.daemonInstanceId;
    if (!runtime || !rpc || !daemonInstanceId) return;
    await rpc.request<null>("fleximark/reconfigureWorkspace", {
      daemonInstanceId,
      workspaceUri: workspace.uri.toString(),
      trusted: vscode.workspace.isTrusted,
    });
  }

  dispose(): void {
    this.#stopping = true;
    for (const runtime of this.#runtimes.values()) this.#stop(runtime);
    this.#runtimes.clear();
    this.#stopDaemon();
    this.#daemon.recoveryStatus?.dispose();
    this.#diagnostics.dispose();
    this.#output.dispose();
  }

  /** Test-only hook returned from activate() while running under ExtensionMode.Test. */
  crashDaemonForTest(): void {
    if (this.#context.extensionMode !== vscode.ExtensionMode.Test)
      throw new Error("daemon crash hook is only available in extension tests");
    if (!this.#daemon.process)
      throw new Error("FlexiMark daemon is unavailable");
    this.#daemon.process.kill();
  }

  /** Test-only snapshot used to verify that crash recovery replaced daemon sessions. */
  recoveryStateForTest(): AdapterRecoveryState {
    if (this.#context.extensionMode !== vscode.ExtensionMode.Test)
      throw new Error("recovery state is only available in extension tests");
    return {
      connectionGeneration: this.#daemon.connectionGeneration,
      daemonInstanceId: this.#daemon.daemonInstanceId,
      documentSessions: Object.fromEntries(
        [...this.#runtimes.values()].flatMap((runtime) =>
          [...runtime.documents].map(([uri, state]) => [uri, state.sessionId]),
        ),
      ),
      previewSessions: Object.fromEntries(
        [...this.#runtimes.values()].flatMap((runtime) =>
          [...runtime.previews.values()].map((preview) => [
            preview.documentUri,
            preview.previewSessionId,
          ]),
        ),
      ),
    };
  }

  removeWorkspace(workspace: vscode.WorkspaceFolder): void {
    const key = workspace.uri.toString();
    const runtime = this.#runtimes.get(key);
    if (!runtime || !this.#runtimes.delete(key)) return;
    this.#daemon.workspaceRevision += 1;
    runtime.removed = true;
    this.#stop(runtime);
    if (this.#runtimes.size === 0) this.#stopDaemon();
    else
      void this.#ensureDaemon().catch((error: unknown) => this.#report(error));
  }

  #runtimeForDocument(
    document: vscode.TextDocument,
  ): WorkspaceRuntime | undefined {
    const workspace = vscode.workspace.getWorkspaceFolder(document.uri);
    return workspace && this.#runtimes.get(workspace.uri.toString());
  }

  #stop(runtime: WorkspaceRuntime): void {
    for (const state of runtime.documents.values()) {
      if (state.checkpoint) clearTimeout(state.checkpoint);
    }
    for (const [uri] of runtime.documents)
      this.#daemon.rpc?.notify("textDocument/didClose", {
        textDocument: { uri },
      });
    for (const preview of runtime.previews.values()) {
      void this.#disposePreview(runtime, preview);
    }
    runtime.documents.clear();
    runtime.previews.clear();
  }

  #stopDaemon(): void {
    if (this.#daemon.restartTimer) {
      clearTimeout(this.#daemon.restartTimer);
      this.#daemon.restartTimer = undefined;
    }
    const { rpc, process: child } = this.#daemon;
    this.#daemon.rpc = undefined;
    this.#daemon.process = undefined;
    this.#daemon.daemonInstanceId = undefined;
    if (rpc) {
      void rpc
        .request("shutdown", undefined, 1_000)
        .catch(() => undefined)
        .finally(() => {
          rpc.notify("exit");
          rpc.close();
          child?.kill();
        });
    } else {
      child?.kill();
    }
  }

  async #ensureDaemon(): Promise<void> {
    if (this.#daemon.restartTimer) {
      clearTimeout(this.#daemon.restartTimer);
      this.#daemon.restartTimer = undefined;
    }
    while (this.#runtimes.size) {
      const pending = this.#daemon.startPromise;
      if (pending) {
        await pending;
        continue;
      }
      if (
        this.#daemon.rpc &&
        this.#daemon.daemonInstanceId &&
        this.#daemon.appliedWorkspaceRevision === this.#daemon.workspaceRevision
      )
        return;

      const operation = (
        this.#daemon.process ? this.#restartDaemon() : this.#launch()
      ).catch((error: unknown) => {
        const rpc = this.#daemon.rpc;
        const child = this.#daemon.process;
        this.#daemon.rpc = undefined;
        this.#daemon.process = undefined;
        this.#daemon.daemonInstanceId = undefined;
        rpc?.close(error instanceof Error ? error : new Error(String(error)));
        child?.kill();
        if (this.#daemon.recoveryId && !this.#stopping && this.#runtimes.size)
          this.#scheduleRecovery(
            `launch failed: ${error instanceof Error ? error.message : String(error)}`,
          );
        throw error;
      });
      this.#daemon.startPromise = operation;
      try {
        await operation;
      } finally {
        if (this.#daemon.startPromise === operation)
          this.#daemon.startPromise = undefined;
      }
    }
  }

  async #restartDaemon(): Promise<void> {
    const child = this.#daemon.process;
    if (!child) return this.#launch();
    this.#daemon.restarting = true;
    const exited = new Promise<void>((resolve) => child.once("exit", resolve));
    this.#daemon.rpc?.close(new Error("FlexiMark daemon is restarting"));
    child.kill();
    await exited;
    this.#daemon.rpc = undefined;
    this.#daemon.process = undefined;
    this.#daemon.daemonInstanceId = undefined;
    for (const runtime of this.#runtimes.values())
      for (const state of runtime.documents.values())
        state.sessionId = undefined;
    this.#daemon.restarting = false;
    await this.#launch();
  }

  async #launch(): Promise<void> {
    const config = vscode.workspace.getConfiguration("fleximark");
    const configuredPath = config.get<string>("daemonPath");
    const binary = configuredPath || (await this.#verifiedBundledDaemon());
    const child = spawn(binary, ["lsp"], {
      cwd: this.#runtimes.values().next().value?.workspace.uri.fsPath,
      stdio: ["pipe", "pipe", "pipe"],
      windowsHide: true,
    });
    this.#daemon.process = child;
    const rpc = new JsonRpcConnection(child.stdout, child.stdin);
    const generation = ++this.#daemon.connectionGeneration;
    const recoveryId =
      this.#daemon.recoveryId ?? `launch-${this.#daemon.recoverySequence + 1}`;
    this.#log(
      `[${recoveryId}] launching daemon generation=${generation} workspaces=${this.#runtimes.size} workspaceRevision=${this.#daemon.workspaceRevision}`,
    );
    this.#daemon.rpc = rpc;
    child.stderr.setEncoding("utf8");
    child.stderr.on("data", (value: string) => {
      if (config.get<string>("logLevel", "info") !== "off") {
        this.#output.append(
          value.replace(
            /(token|secret|authorization)([=: ]+)\S+/gi,
            "$1$2<redacted>",
          ),
        );
      }
    });
    rpc.on("message", (message: JsonRpcRequest) => {
      void this.#handleDaemonMessage(message, rpc, generation);
    });
    child.once("exit", (code, signal) =>
      this.#daemonExited(child, code, signal),
    );
    await new Promise<void>((resolve, reject) => {
      child.once("spawn", resolve);
      child.once("error", reject);
    });
    await rpc.request("initialize", {
      processId: process.pid,
      clientInfo: {
        name: "FlexiMark VS Code",
        version: this.#context.extension.packageJSON.version,
      },
      rootUri: null,
      capabilities: { general: { positionEncodings: ["utf-16"] } },
    });
    rpc.notify("initialized", {});
    const workspaceRevision = this.#daemon.workspaceRevision;
    await this.#initializeWorkspaces(rpc);
    if (this.#daemon.process !== child) return;
    this.#daemon.appliedWorkspaceRevision = workspaceRevision;
    this.#daemon.lastStartedAt = Date.now();
    for (const runtime of this.#runtimes.values())
      await this.#recreatePreviews(runtime);
    const documentCount = [...this.#runtimes.values()].reduce(
      (count, runtime) => count + runtime.documents.size,
      0,
    );
    const previewCount = [...this.#runtimes.values()].reduce(
      (count, runtime) => count + runtime.previews.size,
      0,
    );
    this.#log(
      `[${recoveryId}] daemon ready generation=${generation}; replayed documents=${documentCount} previews=${previewCount}`,
    );
    if (this.#daemon.recoveryId) {
      this.#daemon.recoveryId = undefined;
      this.#daemon.recoveryStatus?.dispose();
      this.#daemon.recoveryStatus = vscode.window.setStatusBarMessage(
        vscode.l10n.t("FlexiMark recovered"),
        3_000,
      );
    }
  }

  async #verifiedBundledDaemon(): Promise<string> {
    const extensionRoot = this.#context.extensionUri.fsPath;
    const manifest = JSON.parse(
      await readFile(path.join(extensionRoot, "bin", "manifest.json"), "utf8"),
    ) as ReleaseManifest;
    if (manifest.schemaVersion !== 1 || !Array.isArray(manifest.artifacts))
      throw new Error("Unsupported FlexiMark release manifest");
    const artifact = manifest.artifacts.find(
      (item) =>
        item.platform === process.platform && item.arch === process.arch,
    );
    if (!artifact)
      throw new Error(
        `FlexiMark does not support ${process.platform}-${process.arch}`,
      );
    const binary = path.resolve(extensionRoot, ...artifact.path.split("/"));
    const relativeBinary = path.relative(extensionRoot, binary);
    if (relativeBinary.startsWith("..") || path.isAbsolute(relativeBinary))
      throw new Error("FlexiMark release manifest path escapes the extension");
    if (!/^[0-9a-f]{64}$/.test(artifact.sha256))
      throw new Error("FlexiMark release manifest has an invalid checksum");
    const checksum = createHash("sha256")
      .update(await readFile(binary))
      .digest("hex");
    if (checksum !== artifact.sha256)
      throw new Error(
        "Bundled FlexiMark daemon is corrupt or does not match this extension",
      );
    return binary;
  }

  async #initializeWorkspaces(rpc: JsonRpcConnection): Promise<void> {
    const initialized = await rpc.request<InitializeResult>(
      "fleximark/initialize",
      {
        protocolVersion,
        client: {
          name: "vscode",
          version: this.#context.extension.packageJSON.version,
        },
        capabilities: {
          embeddedHtml: true,
          structuredPreview: false,
          selectionEvents: true,
          viewportEvents: true,
          openExternal: true,
        },
        workspaces: [...this.#runtimes.values()].map(({ workspace }) => ({
          uri: workspace.uri.toString(),
          trusted: vscode.workspace.isTrusted,
        })),
      },
    );
    if (initialized.protocolVersion !== protocolVersion) {
      throw new Error(
        `Unsupported FlexiMark protocol version ${initialized.protocolVersion}`,
      );
    }
    this.#daemon.daemonInstanceId = initialized.daemonInstanceId;
    for (const status of initialized.workspaceStatuses)
      if (!status.enabled)
        this.#output.appendLine(
          `FlexiMark workspace disabled: ${status.error ?? "invalid configuration"}`,
        );
    for (const runtime of this.#runtimes.values())
      for (const document of vscode.workspace.textDocuments)
        if (
          vscode.workspace.getWorkspaceFolder(document.uri)?.uri.toString() ===
          runtime.workspace.uri.toString()
        )
          await this.#syncDocument(runtime, document);
  }

  async #requestPreview(
    runtime: WorkspaceRuntime,
    document: vscode.TextDocument,
    target: PreviewTarget,
  ): Promise<CreatePreviewResult | undefined> {
    const state = runtime.documents.get(document.uri.toString());
    if (
      !state?.sessionId ||
      !this.#daemon.rpc ||
      !this.#daemon.daemonInstanceId
    )
      return;
    await this.#checkpoint(runtime, document, state);
    return this.#daemon.rpc.request<CreatePreviewResult>(
      "fleximark/createPreview",
      {
        daemonInstanceId: this.#daemon.daemonInstanceId,
        documentSessionId: state.sessionId,
        expectedDocumentVersion: document.version,
        target,
      },
    );
  }

  async #checkpoint(
    runtime: WorkspaceRuntime,
    document: vscode.TextDocument,
    state: DocumentState,
  ): Promise<void> {
    if (!this.#daemon.rpc || !this.#daemon.daemonInstanceId || !state.sessionId)
      return;
    await this.#daemon.rpc.request("fleximark/checkpointDocument", {
      daemonInstanceId: this.#daemon.daemonInstanceId,
      documentSessionId: state.sessionId,
      documentVersion: document.version,
      contentHash: createHash("sha256")
        .update(document.getText())
        .digest("hex"),
    });
  }

  async #recreatePreviews(runtime: WorkspaceRuntime): Promise<void> {
    for (const [previousId, preview] of [...runtime.previews]) {
      const document = vscode.workspace.textDocuments.find(
        (item) => item.uri.toString() === preview.documentUri,
      );
      if (!document) {
        void this.#disposePreview(runtime, preview);
        continue;
      }
      try {
        const result = await this.#requestPreview(
          runtime,
          document,
          preview.target,
        );
        if (!result || !runtime.previews.delete(previousId)) continue;
        preview.previewSessionId = result.previewSessionId;
        preview.url = result.url;
        preview.initialPublication = result.initialPublication;
        preview.renderRevision = result.initialPublication.resultRenderRevision;
        runtime.previews.set(result.previewSessionId, preview);
        if (preview.panel) {
          await preview.panel.webview.postMessage({
            type: "initializePreview",
            publication: result.initialPublication,
          });
        } else {
          if (!result.url)
            throw new Error("daemon omitted the external preview URL");
          await vscode.env.openExternal(vscode.Uri.parse(result.url));
        }
      } catch (error) {
        this.#report(error);
      }
    }
  }

  async #handleDaemonMessage(
    message: JsonRpcRequest,
    connection: JsonRpcConnection,
    generation: number,
  ): Promise<void> {
    if (
      connection !== this.#daemon.rpc ||
      generation !== this.#daemon.connectionGeneration
    )
      return;
    if (message.method === "fleximark/previewEvent") {
      const event = message.params as PreviewEvent;
      if (event.daemonInstanceId !== this.#daemon.daemonInstanceId) return;
      const found = [...this.#runtimes.values()].find((runtime) =>
        runtime.previews.has(event.previewSessionId),
      );
      const preview = found?.previews.get(event.previewSessionId);
      if (!preview) return;
      if (
        event.event.type === "selectSource" ||
        event.event.type === "revealSource"
      ) {
        await this.#applySourceNavigation(preview, event.event);
        return;
      }
      if (event.event.type === "full" || event.event.type === "patch")
        preview.renderRevision = event.event.resultRenderRevision;
      await preview.panel?.webview.postMessage({
        type: "previewEvent",
        event: event.event,
      });
      return;
    }
    if (message.method === "fleximark/requestFullText") {
      const params = message.params as RequestFullTextParams;
      const state = [...this.#runtimes.values()]
        .map((runtime) => runtime.documents.get(params.uri))
        .find(Boolean);
      if (
        params.daemonInstanceId !== this.#daemon.daemonInstanceId ||
        state?.sessionId !== params.documentSessionId
      )
        return;
      const document = vscode.workspace.textDocuments.find(
        (item) => item.uri.toString() === params.uri,
      );
      if (document) {
        connection.notify("textDocument/didChange", {
          textDocument: { uri: params.uri, version: document.version },
          contentChanges: [{ text: document.getText() }],
        });
      }
      if (message.id !== undefined) connection.respond(message.id, null);
      return;
    }
    if (message.method === "textDocument/publishDiagnostics") {
      const params = message.params as {
        uri: string;
        diagnostics: {
          range: {
            start: { line: number; character: number };
            end: { line: number; character: number };
          };
          severity?: number;
          message: string;
          code?: string;
          source?: string;
          data?: unknown;
        }[];
      };
      this.#diagnostics.set(
        vscode.Uri.parse(params.uri),
        params.diagnostics.map((item) => {
          const diagnostic = new vscode.Diagnostic(
            new vscode.Range(
              item.range.start.line,
              item.range.start.character,
              item.range.end.line,
              item.range.end.character,
            ),
            item.message,
            item.severity === 1
              ? vscode.DiagnosticSeverity.Error
              : vscode.DiagnosticSeverity.Warning,
          );
          diagnostic.code = item.code;
          diagnostic.source = item.source;
          (diagnostic as vscode.Diagnostic & { data?: unknown }).data =
            item.data;
          return diagnostic;
        }),
      );
    }
  }

  async #applySourceNavigation(
    preview: PreviewState,
    event: SourceNavigationEvent,
  ): Promise<void> {
    const document = vscode.workspace.textDocuments.find(
      (item) => item.uri.toString() === preview.documentUri,
    );
    if (!document) return;
    const { start, end } = event.sourceRange;
    const range = new vscode.Range(
      start.line,
      sourcePositionToCharacter(document.lineAt(start.line).text, start),
      end.line,
      sourcePositionToCharacter(document.lineAt(end.line).text, end),
    );
    const editor = await vscode.window.showTextDocument(document, {
      preserveFocus: event.type === "revealSource",
      preview: false,
    });
    if (event.type === "selectSource") {
      const selection = new vscode.Selection(range.start, range.end);
      const selectionEcho = {
        uri: preview.documentUri,
        value: `${selection.anchor.line}:${selection.anchor.character}-${selection.active.line}:${selection.active.character}`,
      };
      this.#selectionEcho = selectionEcho;
      editor.selection = selection;
      setTimeout(() => {
        if (this.#selectionEcho === selectionEcho)
          this.#selectionEcho = undefined;
      }, 500);
      const viewportEcho = { uri: preview.documentUri };
      this.#viewportEcho = viewportEcho;
      editor.revealRange(
        range,
        vscode.TextEditorRevealType.InCenterIfOutsideViewport,
      );
      setTimeout(() => {
        if (this.#viewportEcho === viewportEcho) this.#viewportEcho = undefined;
      }, 500);
      return;
    }
    const viewportEcho = { uri: preview.documentUri };
    this.#viewportEcho = viewportEcho;
    editor.revealRange(range, vscode.TextEditorRevealType.AtTop);
    setTimeout(() => {
      if (this.#viewportEcho === viewportEcho) this.#viewportEcho = undefined;
    }, 500);
  }

  async #reloadPreview(
    runtime: WorkspaceRuntime,
    preview: PreviewState,
  ): Promise<void> {
    try {
      await this.#daemon.rpc?.request<unknown>("fleximark/reloadPreview", {
        daemonInstanceId: this.#daemon.daemonInstanceId,
        previewSessionId: preview.previewSessionId,
      });
    } catch (error) {
      this.#output.appendLine(`preview reload failed: ${String(error)}`);
    }
  }

  async #disposePreview(
    runtime: WorkspaceRuntime,
    preview: PreviewState,
  ): Promise<void> {
    if (!runtime.previews.delete(preview.previewSessionId)) return;
    try {
      await this.#daemon.rpc?.request<unknown>("fleximark/disposePreview", {
        daemonInstanceId: this.#daemon.daemonInstanceId,
        previewSessionId: preview.previewSessionId,
      });
    } catch (error) {
      this.#output.appendLine(`preview disposal failed: ${String(error)}`);
    }
    if (preview.panel) preview.panel.dispose();
  }

  #daemonExited(
    process: ChildProcessWithoutNullStreams,
    code: number | null,
    signal: NodeJS.Signals | null,
  ): void {
    if (this.#daemon.process !== process || this.#stopping) return;
    this.#daemon.rpc?.close(new Error("FlexiMark daemon exited"));
    this.#daemon.rpc = undefined;
    this.#daemon.process = undefined;
    this.#daemon.daemonInstanceId = undefined;
    for (const runtime of this.#runtimes.values())
      for (const state of runtime.documents.values())
        state.sessionId = undefined;
    if (this.#daemon.restarting) return;
    if (Date.now() - this.#daemon.lastStartedAt > 30_000)
      this.#daemon.restartCount = 0;
    const recoveryId = `recovery-${++this.#daemon.recoverySequence}`;
    this.#daemon.recoveryId = recoveryId;
    this.#scheduleRecovery(
      `daemon exited code=${code ?? "none"} signal=${signal ?? "none"}`,
    );
  }

  #scheduleRecovery(reason: string): void {
    const recoveryId =
      this.#daemon.recoveryId ?? `recovery-${++this.#daemon.recoverySequence}`;
    this.#daemon.recoveryId = recoveryId;
    this.#daemon.restartCount += 1;
    this.#log(
      `[${recoveryId}] ${reason}; attempt=${this.#daemon.restartCount}`,
    );
    this.#daemon.recoveryStatus?.dispose();
    this.#daemon.recoveryStatus = vscode.window.setStatusBarMessage(
      vscode.l10n.t("FlexiMark daemon stopped; reconnecting…"),
    );
    if (this.#daemon.restartCount > 5) {
      this.#daemon.recoveryStatus?.dispose();
      this.#daemon.recoveryStatus = undefined;
      void vscode.window
        .showErrorMessage(
          vscode.l10n.t("FlexiMark daemon repeatedly failed."),
          vscode.l10n.t("Retry"),
          vscode.l10n.t("Open Output"),
        )
        .then((choice) => {
          if (choice === vscode.l10n.t("Open Output")) {
            this.#output.show(true);
            return;
          }
          if (choice === vscode.l10n.t("Retry")) {
            this.#daemon.restartCount = 0;
            void this.#ensureDaemon().catch((error: unknown) =>
              this.#report(error),
            );
          }
        });
      return;
    }
    const delay = Math.min(250 * 2 ** (this.#daemon.restartCount - 1), 4_000);
    this.#log(`[${recoveryId}] retry scheduled in ${delay}ms`);
    if (this.#daemon.restartTimer) clearTimeout(this.#daemon.restartTimer);
    this.#daemon.restartTimer = setTimeout(() => {
      this.#daemon.restartTimer = undefined;
      void this.#ensureDaemon().catch((error: unknown) => {
        const message = error instanceof Error ? error.message : String(error);
        this.#log(`[${recoveryId}] retry failed: ${message}`);
      });
    }, delay);
  }

  report(error: unknown): void {
    this.#report(error);
  }

  #report(error: unknown): void {
    const message = error instanceof Error ? error.message : String(error);
    this.#output.appendLine(
      message.replace(
        /(token|secret|authorization)([=: ]+)\S+/gi,
        "$1$2<redacted>",
      ),
    );
    void vscode.window.showErrorMessage(
      vscode.l10n.t("FlexiMark failed: {0}", message),
    );
  }

  #log(message: string): void {
    if (
      vscode.workspace
        .getConfiguration("fleximark")
        .get<string>("logLevel", "info") !== "off"
    )
      this.#output.appendLine(message);
  }
}
