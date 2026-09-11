import { type ChildProcessWithoutNullStreams, spawn } from "node:child_process";
import { createHash, randomBytes } from "node:crypto";
import * as path from "node:path";
import * as vscode from "vscode";

import defaultPreviewCss from "../../../web/preview-client/fleximark.css";
import type {
  RenderPublication,
  SourcePosition,
} from "../../../web/preview-client/index.mjs";
import {
  DaemonSupervisor,
  type DaemonSupervisorDependencies,
} from "./daemon-supervisor.mjs";
import { errorMessage, redactSensitiveText } from "./error-policy.mjs";
import { sourcePositionWithinLine } from "./position.mjs";
import { sourcePositionToCharacter } from "./position.mjs";
import {
  type CommandResult,
  type CreatePreviewResult,
  type ExecuteCommandParams,
  type GetNoteOptionsParams,
  type GetNoteOptionsResult,
  type LspMethod,
  type PreviewEvent,
  type PreviewTarget,
  type RequestFullTextParams,
  type SourceNavigationEvent,
  isWebviewInboundMessage,
  protocolVersion,
  shouldForwardEditorNavigation,
} from "./protocol.mjs";
import {
  nodeReleaseManifestEnvironment,
  verifiedBundledDaemon,
} from "./release-manifest.mjs";
import {
  JsonRpcConnection,
  type JsonRpcRequest,
  JsonRpcResponseError,
} from "./rpc.mjs";
import {
  findVisibleSourceEditor,
  previewEventAction,
  selectWorkspaceUriWithPlaceholder,
} from "./workspace-selection.mjs";

export {
  sourcePositionToCharacter,
  sourcePositionWithinLine,
} from "./position.mjs";
export {
  findVisibleSourceEditor,
  previewEventAction,
} from "./workspace-selection.mjs";

interface DocumentState {
  sessionId?: string;
  version: number;
  syncing?: Promise<void>;
  checkpoint?: NodeJS.Timeout;
}

interface PreviewState {
  documentUri: string;
  sourceViewColumn?: vscode.ViewColumn;
  previewSessionId: string;
  target: PreviewTarget;
  url?: string;
  initialPublication: RenderPublication;
  renderRevision: number;
  renderedRevision?: number;
  messageToken?: string;
  panel?: vscode.WebviewPanel;
}

interface WorkspaceRuntime {
  workspace: vscode.WorkspaceFolder;
  documents: Map<string, DocumentState>;
  previews: Map<string, PreviewState>;
  removed: boolean;
}

type AdapterSupervisor = DaemonSupervisor<
  ChildProcessWithoutNullStreams,
  JsonRpcConnection,
  NodeJS.Timeout,
  vscode.Disposable
>;

export interface AdapterRecoveryState {
  connectionGeneration: number;
  daemonInstanceId?: string;
  documentSessions: Record<string, string | undefined>;
  previewSessions: Record<string, string>;
  previewRenderRevisions: Record<string, number | undefined>;
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
  return selectWorkspaceUriWithPlaceholder(
    activeWorkspaceUri,
    workspaces,
    pick,
    vscode.l10n.t("Select a FlexiMark workspace"),
  );
}

export class FlexiMarkAdapter implements vscode.Disposable {
  readonly #context: vscode.ExtensionContext;
  readonly #output = vscode.window.createOutputChannel("FlexiMark");
  readonly #diagnostics =
    vscode.languages.createDiagnosticCollection("fleximark");
  readonly #runtimes = new Map<string, WorkspaceRuntime>();
  readonly #supervisor: AdapterSupervisor;
  #disposed = false;
  #selectionEcho?: { uri: string; value: string };
  #viewportEcho?: { uri: string };

  constructor(context: vscode.ExtensionContext) {
    this.#context = context;
    const dependencies: DaemonSupervisorDependencies<
      ChildProcessWithoutNullStreams,
      JsonRpcConnection,
      NodeJS.Timeout,
      vscode.Disposable
    > = {
      now: () => Date.now(),
      schedule: (callback, delay) => setTimeout(callback, delay),
      cancel: (timer) => clearTimeout(timer),
      workspaceCount: () => this.#runtimes.size,
      resolveBinary: async () => {
        const configuredPath = vscode.workspace
          .getConfiguration("fleximark")
          .get<string>("daemonPath");
        return configuredPath || this.#verifiedBundledDaemon();
      },
      spawn: (binary) =>
        spawn(binary, ["lsp"], {
          cwd: this.#runtimes.values().next().value?.workspace.uri.fsPath,
          stdio: ["pipe", "pipe", "pipe"],
          windowsHide: true,
        }),
      createRpc: (child) => new JsonRpcConnection(child.stdout, child.stdin),
      waitForSpawn: (child) =>
        new Promise<void>((resolve, reject) => {
          child.once("spawn", resolve);
          child.once("error", reject);
        }),
      waitForExit: (child) =>
        new Promise<void>((resolve) => child.once("exit", resolve)),
      onExit: (child, listener) => child.once("exit", listener),
      onRpcClose: (rpc, listener) => rpc.on("close", listener),
      rpcClosed: (rpc) => rpc.closed,
      closeRpc: (rpc, reason) => rpc.close(reason),
      kill: (child) => void child.kill(),
      bindConnection: (child, rpc, generation) =>
        this.#bindDaemonConnection(child, rpc, generation),
      initializeProtocol: (rpc) => this.#initializeProtocol(rpc),
      initializeWorkspaces: (rpc) => this.#initializeWorkspaces(rpc),
      replayDocuments: () => this.#replayDocuments(),
      replayPreviews: () => this.#replayPreviews(),
      resetAdapterState: () => this.#resetDocumentSessions(),
      requestShutdown: (rpc) => rpc.requestLsp("shutdown", undefined, 1_000),
      notifyExit: (rpc) => rpc.notifyLsp("exit"),
      log: (message) => this.#log(message),
      report: (error) => this.#report(error),
      showRecoveryStatus: () =>
        vscode.window.setStatusBarMessage(
          vscode.l10n.t("FlexiMark daemon stopped; reconnecting…"),
        ),
      showRecoveredStatus: () =>
        vscode.window.setStatusBarMessage(
          vscode.l10n.t("FlexiMark recovered"),
          3_000,
        ),
      showRepeatedFailure: async () => {
        const choice = await vscode.window.showErrorMessage(
          vscode.l10n.t("FlexiMark daemon repeatedly failed."),
          vscode.l10n.t("Retry"),
          vscode.l10n.t("Open Output"),
        );
        if (choice === vscode.l10n.t("Retry")) return "retry";
        if (choice === vscode.l10n.t("Open Output")) return "openOutput";
        return undefined;
      },
      openOutput: () => this.#output.show(true),
      disposeStatus: (status) => status.dispose(),
    };
    this.#supervisor = new DaemonSupervisor(dependencies);
  }

  async start(
    workspace?: vscode.WorkspaceFolder,
  ): Promise<WorkspaceRuntime | undefined> {
    if (!workspace || this.#disposed) return;
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
      this.#supervisor.workspaceChanged();
    }
    try {
      await this.#supervisor.ensure();
    } catch (error) {
      if (this.#disposed) return;
      throw error;
    }
    if (this.#disposed) return;
    return runtime;
  }

  async activateDocument(document?: vscode.TextDocument): Promise<void> {
    if (this.#disposed) return;
    const workspace = document
      ? vscode.workspace.getWorkspaceFolder(document.uri)
      : undefined;
    if (!document || !workspace) return;
    await this.start(workspace);
    if (this.#disposed) return;
    if (document?.languageId === "markdown") await this.syncDocument(document);
  }

  async requestLanguageFeature<T>(
    method: LspMethod,
    document: vscode.TextDocument,
    params: object = {},
  ): Promise<T | undefined> {
    await this.activateDocument(document);
    if (this.#disposed) return;
    if (!this.#supervisor.rpc)
      throw new Error("FlexiMark daemon is unavailable");
    const rpc = this.#supervisor.rpc;
    try {
      return await rpc.requestLsp<T>(method, {
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
    const sourceViewColumn = vscode.window.activeTextEditor?.viewColumn;
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
        sourceViewColumn,
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
    const messageToken = randomBytes(32).toString("base64url");
    const shell = `<!doctype html><html><head><meta charset="utf-8"><meta http-equiv="Content-Security-Policy" content="default-src 'none'; script-src 'nonce-${nonce}' ${panel.webview.cspSource}; style-src ${panel.webview.cspSource} 'unsafe-inline'; img-src ${panel.webview.cspSource} data: blob:; media-src ${panel.webview.cspSource} blob:; frame-src https://www.youtube-nocookie.com; object-src 'none';"><meta name="viewport" content="width=device-width,initial-scale=1"><meta name="fleximark-message-token" content="${messageToken}"><style>${defaultPreviewCss}</style></head><body><main id="preview" class="markdown-body"></main><script nonce="${nonce}" src="${scriptUri}"></script></body></html>`;
    const preview: PreviewState = {
      documentUri: document.uri.toString(),
      sourceViewColumn,
      previewSessionId: result.previewSessionId,
      target,
      url: result.url,
      initialPublication: result.initialPublication,
      renderRevision: result.initialPublication.resultRenderRevision,
      messageToken,
      panel,
    };
    runtime.previews.set(result.previewSessionId, preview);
    this.#log(`embedded preview created revision=${preview.renderRevision}`);
    panel.webview.onDidReceiveMessage((event: unknown) => {
      if (!isWebviewInboundMessage(event)) return;
      if (event.type === "ready") {
        this.#log("embedded preview webview ready");
        void panel.webview
          .postMessage({
            type: "initializePreview",
            messageToken,
            publication: preview.initialPublication,
          })
          .then((delivered) =>
            this.#log(
              `embedded preview initial publication delivered=${delivered}`,
            ),
          );
        return;
      }
      if (
        event.type === "rendered" &&
        event.previewSessionId === preview.previewSessionId &&
        event.renderRevision === preview.renderRevision
      ) {
        preview.renderedRevision = event.renderRevision;
        this.#log(`embedded preview rendered revision=${event.renderRevision}`);
        return;
      }
      if (event.type === "requestSnapshot") {
        void this.#reloadPreview(runtime, preview);
        return;
      }
      if (
        shouldForwardEditorNavigation(
          event,
          preview.previewSessionId,
          preview.renderRevision,
        )
      ) {
        if (!this.#supervisor.daemonInstanceId) return;
        this.#supervisor.rpc?.notify("fleximark/previewEvent", {
          daemonInstanceId: this.#supervisor.daemonInstanceId,
          previewSessionId: event.previewSessionId,
          renderRevision: event.renderRevision,
          event,
        });
      }
    });
    panel.onDidDispose(() => void this.#disposePreview(runtime, preview));
    panel.webview.html = shell;
  }

  async syncDocument(document: vscode.TextDocument): Promise<void> {
    if (this.#disposed) return;
    if (document.languageId !== "markdown") return;
    const workspace = vscode.workspace.getWorkspaceFolder(document.uri);
    const runtime = await this.start(workspace);
    if (this.#disposed) return;
    if (!runtime || !this.#supervisor.rpc || !this.#supervisor.daemonInstanceId)
      return;
    await this.#syncDocument(runtime, document);
  }

  async #syncDocument(
    runtime: WorkspaceRuntime,
    document: vscode.TextDocument,
  ): Promise<void> {
    const uri = document.uri.toString();
    const rpc = this.#supervisor.rpc;
    const daemonInstanceId = this.#supervisor.daemonInstanceId;
    if (!rpc || !daemonInstanceId) return;
    const existing = runtime.documents.get(uri);
    if (existing?.sessionId) return;
    if (existing?.syncing) return existing.syncing;
    const state: DocumentState = existing ?? { version: document.version };
    state.syncing = (async () => {
      const version = document.version;
      const text = document.getText();
      rpc.notifyLsp("textDocument/didOpen", {
        textDocument: { uri, languageId: document.languageId, version, text },
      });
      const attached = await rpc.request("fleximark/attachDocument", {
        daemonInstanceId,
        uri,
        expectedDocumentVersion: version,
        contentHash: createHash("sha256").update(text).digest("hex"),
      });
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
    if (!state?.sessionId || !runtime || !this.#supervisor.rpc) return;
    state.version = document.version;
    this.#supervisor.rpc?.notifyLsp("textDocument/didChange", {
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
    this.#supervisor.rpc?.notifyLsp("textDocument/didClose", {
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
    if (!state?.sessionId || !this.#supervisor.daemonInstanceId) return;
    this.#supervisor.rpc?.notify("fleximark/setSelection", {
      daemonInstanceId: this.#supervisor.daemonInstanceId,
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
    if (!state?.sessionId || !this.#supervisor.daemonInstanceId) return;
    this.#supervisor.rpc?.notify("fleximark/setViewport", {
      daemonInstanceId: this.#supervisor.daemonInstanceId,
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
    const workspaceUri = await selectWorkspaceUriWithPlaceholder(
      activeWorkspace?.uri.toString(),
      folders.map((folder) => ({
        label: folder.name,
        uri: folder.uri.toString(),
      })),
      vscode.window.showQuickPick,
      vscode.l10n.t("Select a FlexiMark workspace"),
    );
    if (!workspaceUri) return;
    const workspace = folders.find(
      (folder) => folder.uri.toString() === workspaceUri,
    );
    const runtime = await this.start(workspace);
    if (!runtime) return;
    if (document?.languageId === "markdown") await this.syncDocument(document);
    const state = document && runtime.documents.get(document.uri.toString());
    const rpc = this.#supervisor.rpc;
    if (!this.#supervisor.daemonInstanceId || !rpc) return;
    const params: ExecuteCommandParams = {
      daemonInstanceId: this.#supervisor.daemonInstanceId,
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
              rpc.request("fleximark/getNoteOptions", optionParams),
            vscode.window.showQuickPick,
            (commandParams) =>
              rpc.request("fleximark/executeCommand", commandParams),
          )
        : await rpc.request("fleximark/executeCommand", params);
    if (!result) return;
    await openCommandResult(
      params,
      result,
      async (uri) => {
        await vscode.window.showTextDocument(vscode.Uri.parse(uri));
      },
      (acknowledgement) =>
        rpc.request("fleximark/executeCommand", acknowledgement),
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
    const rpc = this.#supervisor.rpc;
    const daemonInstanceId = this.#supervisor.daemonInstanceId;
    if (!runtime || !rpc || !daemonInstanceId) return;
    await rpc.request("fleximark/reconfigureWorkspace", {
      daemonInstanceId,
      workspaceUri: workspace.uri.toString(),
      trusted: vscode.workspace.isTrusted,
    });
  }

  dispose(): void {
    if (this.#disposed) return;
    this.#disposed = true;
    this.#supervisor.beginDispose();
    for (const runtime of this.#runtimes.values()) this.#stop(runtime);
    this.#runtimes.clear();
    this.#supervisor.dispose();
    this.#diagnostics.dispose();
    this.#output.dispose();
  }

  /** Test-only hook returned from activate() while running under ExtensionMode.Test. */
  crashDaemonForTest(): void {
    if (this.#context.extensionMode !== vscode.ExtensionMode.Test)
      throw new Error("daemon crash hook is only available in extension tests");
    if (!this.#supervisor.process)
      throw new Error("FlexiMark daemon is unavailable");
    this.#supervisor.process.kill();
  }

  /** Test-only snapshot used to verify that crash recovery replaced daemon sessions. */
  recoveryStateForTest(): AdapterRecoveryState {
    if (this.#context.extensionMode !== vscode.ExtensionMode.Test)
      throw new Error("recovery state is only available in extension tests");
    return {
      connectionGeneration: this.#supervisor.connectionGeneration,
      daemonInstanceId: this.#supervisor.daemonInstanceId,
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
      previewRenderRevisions: Object.fromEntries(
        [...this.#runtimes.values()].flatMap((runtime) =>
          [...runtime.previews.values()].map((preview) => [
            preview.documentUri,
            preview.renderedRevision,
          ]),
        ),
      ),
    };
  }

  removeWorkspace(workspace: vscode.WorkspaceFolder): void {
    const key = workspace.uri.toString();
    const runtime = this.#runtimes.get(key);
    if (!runtime || !this.#runtimes.delete(key)) return;
    this.#supervisor.workspaceChanged();
    runtime.removed = true;
    this.#stop(runtime);
    if (this.#runtimes.size === 0) this.#supervisor.stop();
    else
      void this.#supervisor
        .ensure()
        .catch((error: unknown) => this.#report(error));
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
      this.#supervisor.rpc?.notifyLsp("textDocument/didClose", {
        textDocument: { uri },
      });
    for (const preview of runtime.previews.values()) {
      void this.#disposePreview(runtime, preview);
    }
    runtime.documents.clear();
    runtime.previews.clear();
  }

  #bindDaemonConnection(
    child: ChildProcessWithoutNullStreams,
    rpc: JsonRpcConnection,
    generation: number,
  ): void {
    const config = vscode.workspace.getConfiguration("fleximark");
    child.stderr.setEncoding("utf8");
    child.stderr.on("data", (value: string) => {
      if (config.get<string>("logLevel", "info") !== "off")
        this.#appendOutput(value);
    });
    rpc.on("message", (message: JsonRpcRequest) => {
      void this.#handleDaemonMessage(message, rpc, generation).catch(
        (error: unknown) => this.#report(error),
      );
    });
    rpc.on("invalidMessage", (message: JsonRpcRequest) => {
      this.#handleInvalidDaemonMessage(message, rpc, generation);
    });
  }

  async #initializeProtocol(rpc: JsonRpcConnection): Promise<void> {
    await rpc.requestLsp("initialize", {
      processId: process.pid,
      clientInfo: {
        name: "FlexiMark VS Code",
        version: this.#context.extension.packageJSON.version,
      },
      rootUri: null,
      capabilities: { general: { positionEncodings: ["utf-16"] } },
    });
    rpc.notifyLsp("initialized", {});
  }

  async #replayDocuments(): Promise<void> {
    for (const runtime of this.#runtimes.values())
      for (const document of vscode.workspace.textDocuments)
        if (
          vscode.workspace.getWorkspaceFolder(document.uri)?.uri.toString() ===
          runtime.workspace.uri.toString()
        )
          await this.#syncDocument(runtime, document);
  }

  async #replayPreviews(): Promise<{ documents: number; previews: number }> {
    for (const runtime of this.#runtimes.values())
      await this.#recreatePreviews(runtime);
    const documents = [...this.#runtimes.values()].reduce(
      (count, runtime) => count + runtime.documents.size,
      0,
    );
    const previews = [...this.#runtimes.values()].reduce(
      (count, runtime) => count + runtime.previews.size,
      0,
    );
    return { documents, previews };
  }

  #resetDocumentSessions(): void {
    for (const runtime of this.#runtimes.values())
      for (const state of runtime.documents.values())
        state.sessionId = undefined;
  }

  async #verifiedBundledDaemon(): Promise<string> {
    return verifiedBundledDaemon(
      nodeReleaseManifestEnvironment(this.#context.extensionUri.fsPath),
    );
  }

  async #initializeWorkspaces(rpc: JsonRpcConnection): Promise<string> {
    const initialized = await rpc.request("fleximark/initialize", {
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
    });
    if (initialized.protocolVersion !== protocolVersion) {
      throw new Error(
        `Unsupported FlexiMark protocol version ${initialized.protocolVersion}`,
      );
    }
    for (const status of initialized.workspaceStatuses)
      if (!status.enabled)
        this.#appendOutputLine(
          `FlexiMark workspace disabled: ${status.error ?? "invalid configuration"}`,
        );
    return initialized.daemonInstanceId;
  }

  async #requestPreview(
    runtime: WorkspaceRuntime,
    document: vscode.TextDocument,
    target: PreviewTarget,
  ): Promise<CreatePreviewResult | undefined> {
    const state = runtime.documents.get(document.uri.toString());
    if (
      !state?.sessionId ||
      !this.#supervisor.rpc ||
      !this.#supervisor.daemonInstanceId
    )
      return;
    await this.#checkpoint(runtime, document, state);
    return this.#supervisor.rpc.request("fleximark/createPreview", {
      daemonInstanceId: this.#supervisor.daemonInstanceId,
      documentSessionId: state.sessionId,
      expectedDocumentVersion: document.version,
      target,
    });
  }

  async #checkpoint(
    runtime: WorkspaceRuntime,
    document: vscode.TextDocument,
    state: DocumentState,
  ): Promise<void> {
    if (
      !this.#supervisor.rpc ||
      !this.#supervisor.daemonInstanceId ||
      !state.sessionId
    )
      return;
    await this.#supervisor.rpc.request("fleximark/checkpointDocument", {
      daemonInstanceId: this.#supervisor.daemonInstanceId,
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
        preview.renderedRevision = undefined;
        runtime.previews.set(result.previewSessionId, preview);
        if (preview.panel) {
          await preview.panel.webview.postMessage({
            type: "initializePreview",
            messageToken: preview.messageToken,
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
      connection !== this.#supervisor.rpc ||
      generation !== this.#supervisor.connectionGeneration
    )
      return;
    if (message.method === "fleximark/previewEvent") {
      const event = message.params as PreviewEvent;
      if (event.daemonInstanceId !== this.#supervisor.daemonInstanceId) return;
      const found = [...this.#runtimes.values()].find((runtime) =>
        runtime.previews.has(event.previewSessionId),
      );
      const preview = found?.previews.get(event.previewSessionId);
      if (!preview) return;
      const action = previewEventAction(preview.renderRevision, event);
      if (action === "ignore") return;
      if (action === "reload") {
        if (found) void this.#reloadPreview(found, preview);
        return;
      }
      if (
        event.event.type === "selectSource" ||
        event.event.type === "revealSource"
      ) {
        await this.#applySourceNavigation(preview, event.event);
        return;
      }
      if (event.event.type === "full") {
        preview.renderRevision = event.event.resultRenderRevision;
      } else if (event.event.type === "patch") {
        preview.renderRevision = event.event.resultRenderRevision;
      }
      await preview.panel?.webview.postMessage({
        type: "previewEvent",
        messageToken: preview.messageToken,
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
        params.daemonInstanceId !== this.#supervisor.daemonInstanceId ||
        state?.sessionId !== params.documentSessionId
      )
        return;
      const document = vscode.workspace.textDocuments.find(
        (item) => item.uri.toString() === params.uri,
      );
      if (document) {
        connection.notifyLsp("textDocument/didChange", {
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

  #handleInvalidDaemonMessage(
    message: JsonRpcRequest,
    connection: JsonRpcConnection,
    generation: number,
  ): void {
    if (
      connection !== this.#supervisor.rpc ||
      generation !== this.#supervisor.connectionGeneration ||
      message.method !== "fleximark/previewEvent" ||
      !message.params
    )
      return;
    const params = message.params as Record<string, unknown>;
    const event = params.event;
    if (
      params.daemonInstanceId !== this.#supervisor.daemonInstanceId ||
      typeof params.previewSessionId !== "string" ||
      event === null ||
      typeof event !== "object" ||
      !["full", "patch"].includes(
        (event as Record<string, unknown>).type as string,
      )
    )
      return;
    for (const runtime of this.#runtimes.values()) {
      const preview = runtime.previews.get(params.previewSessionId);
      if (preview) {
        void this.#reloadPreview(runtime, preview);
        return;
      }
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
    if (
      !this.#sourcePositionInDocument(document, start) ||
      !this.#sourcePositionInDocument(document, end)
    )
      return;
    const range = new vscode.Range(
      start.line,
      sourcePositionToCharacter(document.lineAt(start.line).text, start),
      end.line,
      sourcePositionToCharacter(document.lineAt(end.line).text, end),
    );
    const editor =
      findVisibleSourceEditor(
        vscode.window.visibleTextEditors,
        preview.documentUri,
        preview.sourceViewColumn,
      ) ??
      (await vscode.window.showTextDocument(document, {
        viewColumn: preview.sourceViewColumn,
        preserveFocus: true,
        preview: false,
      }));
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

  #sourcePositionInDocument(
    document: vscode.TextDocument,
    position: SourcePosition,
  ): boolean {
    if (position.line < 0 || position.line >= document.lineCount) return false;
    const line = document.lineAt(position.line).text;
    return sourcePositionWithinLine(line, position);
  }

  async #reloadPreview(
    runtime: WorkspaceRuntime,
    preview: PreviewState,
  ): Promise<void> {
    try {
      const daemonInstanceId = this.#supervisor.daemonInstanceId;
      if (!daemonInstanceId) return;
      await this.#supervisor.rpc?.request("fleximark/reloadPreview", {
        daemonInstanceId,
        previewSessionId: preview.previewSessionId,
      });
    } catch (error) {
      this.#appendOutputLine(`preview reload failed: ${String(error)}`);
    }
  }

  async #disposePreview(
    runtime: WorkspaceRuntime,
    preview: PreviewState,
  ): Promise<void> {
    if (!runtime.previews.delete(preview.previewSessionId)) return;
    try {
      const daemonInstanceId = this.#supervisor.daemonInstanceId;
      if (daemonInstanceId)
        await this.#supervisor.rpc?.request("fleximark/disposePreview", {
          daemonInstanceId,
          previewSessionId: preview.previewSessionId,
        });
    } catch (error) {
      this.#appendOutputLine(`preview disposal failed: ${String(error)}`);
    }
    if (preview.panel) preview.panel.dispose();
  }

  report(error: unknown): void {
    this.#report(error);
  }

  #report(error: unknown): void {
    const message = errorMessage(error);
    this.#appendOutputLine(message);
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
      this.#appendOutputLine(message);
  }

  #appendOutput(message: string): void {
    if (this.#disposed) return;
    this.#output.append(redactSensitiveText(message));
  }

  #appendOutputLine(message: string): void {
    if (this.#disposed) return;
    this.#output.appendLine(redactSensitiveText(message));
  }
}
