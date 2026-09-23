import { type ChildProcessWithoutNullStreams, spawn } from "node:child_process";
import { randomBytes } from "node:crypto";
import * as vscode from "vscode";

import defaultPreviewCss from "../../../web/preview-client/fleximark.css";
import katexCss from "../../../web/preview-client/katex.css";
import { executeCreateNote, openCommandResult } from "./commands.mjs";
import {
  DaemonSupervisor,
  type DaemonSupervisorDependencies,
} from "./daemon-supervisor.mjs";
import {
  applyPublishedDiagnostics,
  isPublishDiagnosticsParams,
} from "./diagnostics.mjs";
import {
  type DaemonOrigin,
  checkpointDocument,
  changeDocument as coordinateDocumentChange,
  closeDocument as coordinateDocumentClose,
  syncDocument as coordinateDocumentSync,
  handleRequestFullText,
  replayDocumentSessions,
  resetDocumentSessions,
} from "./document-coordinator.mjs";
import { errorMessage, redactSensitiveText } from "./error-policy.mjs";
import { sourcePositionWithinLine } from "./position.mjs";
import { sourcePositionToCharacter } from "./position.mjs";
import {
  type PreviewCandidate,
  type PreviewCandidateCurrentState,
  type PreviewCandidateOrigin,
  type PreviewEchoState,
  type PreviewLifecycleDependencies,
  disposePreviewCandidate,
  disposePreviewLifecycle,
  followActiveDocumentLifecycle,
  handlePreviewChangedLifecycle,
  handlePreviewEventLifecycle,
  openPreviewLifecycle,
  previewCandidateIdentityIsCurrent,
  previewCandidateIsCurrent,
  previewIsCurrent,
  recreatePreviewsLifecycle,
  requestPreviewCandidate,
  rerenderPreviewLifecycle,
  sameDaemonOrigin,
  synchronizePreviewLifecycle,
} from "./preview-coordinator.mjs";
import type { SourcePosition } from "./protocol.mjs";
import {
  type CommandResult,
  type ExecuteCommandParams,
  type LspMethod,
  type PreviewChangedParams,
  type PreviewTarget,
  type ServerPreviewEventParams,
  type SourceNavigationEvent,
  protocolVersion,
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
import type {
  DocumentState,
  PreviewState,
  WorkspaceRuntime,
} from "./runtime-state.mjs";
import {
  findVisibleSourceEditor,
  selectWorkspaceUriWithPlaceholder,
} from "./workspace-selection.mjs";

export {
  sourcePositionToCharacter,
  sourcePositionWithinLine,
} from "./position.mjs";

export { findVisibleSourceEditor } from "./workspace-selection.mjs";
export {
  executeCreateNote,
  openCommandResult,
  selectNoteCategory,
  selectWorkspaceUri,
} from "./commands.mjs";

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

export class FlexiMarkAdapter implements vscode.Disposable {
  readonly #context: vscode.ExtensionContext;
  readonly #output = vscode.window.createOutputChannel("FlexiMark");
  readonly #diagnostics =
    vscode.languages.createDiagnosticCollection("fleximark");
  readonly #runtimes = new Map<string, WorkspaceRuntime>();
  readonly #supervisor: AdapterSupervisor;
  #disposed = false;
  #testOrigin?: DaemonOrigin;
  readonly #previewEchoes: PreviewEchoState = {};
  readonly #previewLifecycleContext: PreviewLifecycleDependencies = {
    activeEditor: () => vscode.window.activeTextEditor,
    showNoDocument: () => this.#showNoDocument(),
    showNoWorkspace: () => this.#showNoWorkspace(),
    workspaceFor: (document) =>
      vscode.workspace.getWorkspaceFolder(document.uri),
    start: (workspace) => this.start(workspace),
    sync: (document) => this.syncDocument(document),
    document: (uri) =>
      vscode.workspace.textDocuments.find(
        (document) => document.uri.toString() === uri,
      ),
    request: (runtime, document, target) =>
      this.#requestPreview(runtime, document, target),
    candidateCurrent: (candidate) => this.#previewCandidateIsCurrent(candidate),
    rejectCandidate: disposePreviewCandidate,
    synchronize: (runtime, preview, force) =>
      this.#synchronizePreview(runtime, preview, force),
    openExternal: async (url) => {
      await vscode.env.openExternal(vscode.Uri.parse(url));
    },
    createPanel: (title) =>
      vscode.window.createWebviewPanel(
        "fleximark.preview",
        title,
        vscode.workspace
          .getConfiguration("fleximark")
          .get<"active" | "beside">("previewColumn", "beside") === "active"
          ? vscode.ViewColumn.Active
          : vscode.ViewColumn.Beside,
        { enableScripts: true, retainContextWhenHidden: true },
      ),
    scriptUri: (panel) =>
      panel.webview
        .asWebviewUri(
          vscode.Uri.joinPath(
            this.#context.extensionUri,
            "dist",
            "web",
            "preview-client",
            "vscode-host.js",
          ),
        )
        .toString(),
    random: (size, encoding) => randomBytes(size).toString(encoding),
    css: `${katexCss}\n${defaultPreviewCss}`,
    log: (message) => this.#log(message),
    previewCurrent: (runtime, preview) =>
      this.#previewIsActive(runtime, preview),
    owner: (preview) => this.#previewOwner(preview),
    report: (error) => this.#report(error),
    dispose: (runtime, preview) => this.#disposePreview(runtime, preview),
    notifyNavigation: (event) => {
      const origin = this.#daemonOrigin();
      origin?.rpc.notify("fleximark/previewEvent", {
        daemonInstanceId: origin.daemonInstanceId,
        ...event,
      });
    },
  };

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

  #showNoDocument(): void {
    void vscode.window.showInformationMessage(
      vscode.l10n.t("Open a Markdown document first."),
    );
  }

  #showNoWorkspace(): void {
    void vscode.window.showInformationMessage(
      vscode.l10n.t("Open the Markdown document inside a workspace first."),
    );
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

  async activateEditor(editor?: vscode.TextEditor): Promise<void> {
    await this.activateDocument(editor?.document);
    if (this.#disposed) return;
    await followActiveDocumentLifecycle(
      editor,
      this.#runtimes.values(),
      this.#previewLifecycleContext,
    );
  }

  async executeMigrationCommand(
    workspace: vscode.WorkspaceFolder,
    command: "inspectLegacyWorkspace" | "migrateWorkspace",
    args: readonly string[] = [],
  ): Promise<CommandResult> {
    if (!(await this.start(workspace)))
      throw new Error("FlexiMark adapter is unavailable");
    const rpc = this.#supervisor.rpc;
    const daemonInstanceId = this.#supervisor.daemonInstanceId;
    if (!rpc || !daemonInstanceId)
      throw new Error("FlexiMark daemon is unavailable");
    const result = await rpc.request("fleximark/executeCommand", {
      daemonInstanceId,
      command,
      workspaceUri: workspace.uri.toString(),
      arguments: [...args],
    });
    if (command === "migrateWorkspace")
      await this.reconfigureWorkspace(workspace);
    return result;
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
    await openPreviewLifecycle(target, this.#previewLifecycleContext);
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
    const origin = this.#daemonOrigin();
    if (!origin) return;
    return coordinateDocumentSync(
      runtime,
      document,
      origin,
      (owner, openedDocument, state, operationOrigin) =>
        !this.#disposed &&
        !owner.removed &&
        this.#runtimes.get(owner.workspace.uri.toString()) === owner &&
        owner.documents.get(openedDocument.uri.toString()) === state &&
        vscode.workspace.textDocuments.includes(openedDocument) &&
        this.#originIsCurrent(operationOrigin),
      (changed) => this.changeDocument(changed),
    );
  }

  changeDocument(document: vscode.TextDocument): void {
    const runtime = this.#runtimeForDocument(document);
    const state = runtime?.documents.get(document.uri.toString());
    const origin = this.#daemonOrigin();
    coordinateDocumentChange(
      runtime,
      document,
      origin?.rpc,
      () => this.syncDocument(document),
      () =>
        runtime && state
          ? this.#checkpoint(document, state, origin)
          : Promise.resolve(),
      (error) => this.#report(error),
    );
  }

  closeDocument(document: vscode.TextDocument): void {
    const runtime = this.#runtimeForDocument(document);
    coordinateDocumentClose(runtime, document, this.#supervisor.rpc);
  }

  selectionChanged(event: vscode.TextEditorSelectionChangeEvent): void {
    const uri = event.textEditor.document.uri.toString();
    const value = event.selections
      .map(
        ({ anchor, active }) =>
          `${anchor.line}:${anchor.character}-${active.line}:${active.character}`,
      )
      .join(",");
    if (
      this.#previewEchoes.selection?.uri === uri &&
      this.#previewEchoes.selection.value === value
    ) {
      this.#previewEchoes.selection = undefined;
      return;
    }
    const state = this.#runtimeForDocument(
      event.textEditor.document,
    )?.documents.get(uri);
    const origin = this.#daemonOrigin();
    if (!state?.sessionId || !origin) return;
    origin.rpc.notify("fleximark/setSelection", {
      daemonInstanceId: origin.daemonInstanceId,
      documentSessionId: state.sessionId,
      expectedDocumentVersion: event.textEditor.document.version,
      selections: event.selections.map(({ anchor, active }) => ({
        anchor: { line: anchor.line, character: anchor.character },
        active: { line: active.line, character: active.character },
      })),
    });
  }

  viewportChanged(event: vscode.TextEditorVisibleRangesChangeEvent): void {
    const document = event.textEditor.document;
    const uri = document.uri.toString();
    if (this.#previewEchoes.viewport?.uri === uri) {
      this.#previewEchoes.viewport = undefined;
      return;
    }
    const state = this.#runtimeForDocument(document)?.documents.get(uri);
    const origin = this.#daemonOrigin();
    if (!state?.sessionId || !origin) return;
    origin.rpc.notify("fleximark/setViewport", {
      daemonInstanceId: origin.daemonInstanceId,
      documentSessionId: state.sessionId,
      expectedDocumentVersion: document.version,
      ranges: event.visibleRanges.map(({ start, end }) => ({
        start: { line: start.line, character: start.character },
        end: { line: end.line, character: end.character },
      })),
    });
  }

  async previewNavigationForTest(
    runtime: WorkspaceRuntime,
    preview: PreviewState,
    event: SourceNavigationEvent,
  ): Promise<void> {
    if (this.#context.extensionMode !== vscode.ExtensionMode.Test)
      throw new Error("preview navigation seam is only available in tests");
    this.#runtimes.set(runtime.workspace.uri.toString(), runtime);
    runtime.previews.set(preview.previewSessionId, preview);
    this.#testOrigin = preview.origin;
    await this.#applySourceNavigation(runtime, preview, event);
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
      workspaceUri,
      ...(state && document
        ? {
            documentSessionId: state.sessionId,
            expectedDocumentVersion: document.version,
          }
        : {}),
    };
    const result =
      command === "createNote"
        ? await executeCreateNote(
            params,
            (optionParams) =>
              rpc.request("fleximark/getNoteOptions", optionParams),
            (items, options) => vscode.window.showQuickPick(items, options),
            (items, options) => vscode.window.showQuickPick(items, options),
            (options) => vscode.window.showInputBox(options),
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
        [...runtime.previews.values()].map(async (preview) => {
          const current = await rerenderPreviewLifecycle(
            runtime,
            preview,
            this.#previewLifecycleContext,
          );
          if (current && preview.panel)
            await this.#synchronizePreview(runtime, preview, true);
        }),
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
            preview.renderRevision || undefined,
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

  #previewOwner(preview: PreviewState): WorkspaceRuntime | undefined {
    return [...this.#runtimes.values()].find(
      (runtime) => runtime.previews.get(preview.previewSessionId) === preview,
    );
  }

  #daemonOrigin(): DaemonOrigin | undefined {
    const rpc = this.#supervisor.rpc;
    const daemonInstanceId = this.#supervisor.daemonInstanceId;
    if (!rpc || rpc.closed || !daemonInstanceId) return this.#testOrigin;
    return {
      rpc,
      daemonInstanceId,
      generation: this.#supervisor.connectionGeneration,
    };
  }

  #originIsCurrent(origin: DaemonOrigin): boolean {
    const current = this.#daemonOrigin();
    return !origin.rpc.closed && sameDaemonOrigin(current, origin);
  }

  #previewCandidateIsCurrent(candidate: PreviewCandidate): boolean {
    return previewCandidateIsCurrent(
      candidate,
      this.#previewCandidateCurrentState(candidate),
    );
  }

  #previewCandidateCurrentState(
    candidate: PreviewCandidateOrigin,
  ): PreviewCandidateCurrentState {
    const document = vscode.workspace.textDocuments.find(
      (item) => item.uri.toString() === candidate.documentUri,
    );
    return {
      disposed: this.#disposed,
      runtime: this.#runtimes.get(candidate.runtime.workspace.uri.toString()),
      origin: this.#daemonOrigin(),
      documentState: candidate.runtime.documents.get(candidate.documentUri),
      documentVersion: document?.version,
    };
  }

  #previewIsActive(runtime: WorkspaceRuntime, preview: PreviewState): boolean {
    const origin = this.#daemonOrigin();
    return (
      !this.#disposed &&
      this.#runtimes.get(runtime.workspace.uri.toString()) === runtime &&
      previewIsCurrent(runtime, preview, preview.previewSessionId) &&
      sameDaemonOrigin(origin, preview.origin)
    );
  }

  #previewCandidateIdentityIsCurrent(
    candidate: PreviewCandidateOrigin,
  ): boolean {
    return previewCandidateIdentityIsCurrent(
      candidate,
      this.#previewCandidateCurrentState(candidate),
    );
  }

  #synchronizePreview(
    runtime: WorkspaceRuntime,
    preview: PreviewState,
    force = false,
  ): Promise<void> {
    return synchronizePreviewLifecycle(
      runtime,
      preview,
      this.#previewLifecycleContext,
      force,
    );
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
  }

  async #initializeProtocol(rpc: JsonRpcConnection): Promise<void> {
    await rpc.requestLsp("initialize", {
      processId: process.pid,
      clientInfo: {
        name: "FlexiMark VS Code",
        version: this.#context.extension.packageJSON.version,
      },
      rootUri: null,
      capabilities: {
        general: { positionEncodings: ["utf-16"] },
        textDocument: {
          completion: { completionItem: { snippetSupport: true } },
          semanticTokens: {
            requests: { full: true },
            tokenTypes: ["keyword", "string", "operator", "type", "property"],
            tokenModifiers: [],
            formats: ["relative"],
            overlappingTokenSupport: false,
            multilineTokenSupport: false,
          },
        },
      },
    });
    rpc.notifyLsp("initialized", {});
  }

  async #replayDocuments(): Promise<void> {
    for (const runtime of this.#runtimes.values())
      await replayDocumentSessions(
        vscode.workspace.textDocuments.filter(
          (document) =>
            vscode.workspace
              .getWorkspaceFolder(document.uri)
              ?.uri.toString() === runtime.workspace.uri.toString(),
        ),
        (document) => this.#syncDocument(runtime, document),
        (document, error) => {
          for (const preview of [...runtime.previews.values()])
            if (preview.documentUri === document.uri.toString())
              void this.#disposePreview(runtime, preview);
          this.#report(error);
        },
      );
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
    resetDocumentSessions(this.#runtimes.values());
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
  ): Promise<PreviewCandidate | undefined> {
    return requestPreviewCandidate(
      runtime,
      document,
      target,
      () => this.#daemonOrigin(),
      (openedDocument, state, origin) =>
        this.#checkpoint(openedDocument, state, origin),
      (candidate) => this.#previewCandidateIdentityIsCurrent(candidate),
      disposePreviewCandidate,
    );
  }

  async #checkpoint(
    document: vscode.TextDocument,
    state: DocumentState,
    origin = this.#daemonOrigin(),
  ): Promise<void> {
    await checkpointDocument(document, state, origin);
  }

  async #recreatePreviews(runtime: WorkspaceRuntime): Promise<void> {
    await recreatePreviewsLifecycle(runtime, this.#previewLifecycleContext);
  }

  async #handleDaemonMessage(
    message: JsonRpcRequest,
    connection: JsonRpcConnection,
    generation: number,
  ): Promise<void> {
    if (
      connection.closed ||
      connection !== this.#supervisor.rpc ||
      generation !== this.#supervisor.connectionGeneration
    )
      return;
    if (message.method === "fleximark/previewEvent") {
      const event = message.params as ServerPreviewEventParams;
      if (event.daemonInstanceId !== this.#supervisor.daemonInstanceId) return;
      const eventOrigin = {
        rpc: connection,
        generation,
        daemonInstanceId: event.daemonInstanceId,
      };
      await handlePreviewEventLifecycle(
        eventOrigin,
        event,
        this.#runtimes.values(),
        (runtime, preview, navigation) =>
          this.#applySourceNavigation(runtime, preview, navigation),
      );
      return;
    }
    if (message.method === "fleximark/previewChanged") {
      const changed = message.params as PreviewChangedParams;
      if (changed.daemonInstanceId !== this.#supervisor.daemonInstanceId)
        return;
      handlePreviewChangedLifecycle(
        {
          rpc: connection,
          generation,
          daemonInstanceId: changed.daemonInstanceId,
        },
        changed,
        this.#runtimes.values(),
        (runtime, preview) => void this.#synchronizePreview(runtime, preview),
      );
      return;
    }
    if (
      handleRequestFullText(
        message,
        connection,
        this.#runtimes.values(),
        this.#supervisor.daemonInstanceId,
        vscode.workspace.textDocuments,
      )
    )
      return;
    if (
      message.method === "textDocument/publishDiagnostics" &&
      isPublishDiagnosticsParams(message.params)
    )
      applyPublishedDiagnostics(this.#diagnostics, message.params);
  }

  async #applySourceNavigation(
    runtime: WorkspaceRuntime,
    preview: PreviewState,
    event: SourceNavigationEvent,
  ): Promise<void> {
    const expectedRevision = preview.renderRevision;
    const expectedSession = preview.previewSessionId;
    const expectedOrigin = preview.origin;
    const document = vscode.workspace.textDocuments.find(
      (item) => item.uri.toString() === preview.documentUri,
    );
    if (!document) return;
    const expectedDocumentVersion = document.version;
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
    if (
      !this.#previewIsActive(runtime, preview) ||
      preview.previewSessionId !== expectedSession ||
      preview.origin !== expectedOrigin ||
      preview.renderRevision !== expectedRevision ||
      !vscode.workspace.textDocuments.includes(document) ||
      document.version !== expectedDocumentVersion ||
      editor.document !== document
    )
      return;
    const clearEcho = (type: keyof PreviewEchoState, token: object) =>
      setTimeout(() => {
        if (this.#previewEchoes[type] === token)
          this.#previewEchoes[type] = undefined;
      }, 500);
    const viewportEcho = { uri: preview.documentUri };
    this.#previewEchoes.viewport = viewportEcho;
    if (event.type === "selectSource") {
      const selection = new vscode.Selection(range.start, range.end);
      const selectionEcho = {
        uri: preview.documentUri,
        value: `${selection.anchor.line}:${selection.anchor.character}-${selection.active.line}:${selection.active.character}`,
      };
      this.#previewEchoes.selection = selectionEcho;
      editor.selection = selection;
      clearEcho("selection", selectionEcho);
      editor.revealRange(
        range,
        vscode.TextEditorRevealType.InCenterIfOutsideViewport,
      );
    } else editor.revealRange(range, vscode.TextEditorRevealType.AtTop);
    clearEcho("viewport", viewportEcho);
  }

  #sourcePositionInDocument(
    document: vscode.TextDocument,
    position: SourcePosition,
  ): boolean {
    if (position.line < 0 || position.line >= document.lineCount) return false;
    const line = document.lineAt(position.line).text;
    return sourcePositionWithinLine(line, position);
  }

  async #disposePreview(
    runtime: WorkspaceRuntime,
    preview: PreviewState,
  ): Promise<void> {
    await disposePreviewLifecycle(runtime, preview, (message) =>
      this.#appendOutputLine(message),
    );
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
