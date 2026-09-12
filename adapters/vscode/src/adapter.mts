import { type ChildProcessWithoutNullStreams, spawn } from "node:child_process";
import { randomBytes } from "node:crypto";
import * as vscode from "vscode";

import defaultPreviewCss from "../../../web/preview-client/fleximark.css";
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
  resetDocumentSessions,
} from "./document-coordinator.mjs";
import { errorMessage, redactSensitiveText } from "./error-policy.mjs";
import { sourcePositionWithinLine } from "./position.mjs";
import { sourcePositionToCharacter } from "./position.mjs";
import {
  type PreviewCandidate,
  type PreviewCandidateOrigin,
  type PreviewEchoState,
  type PreviewReadinessDependencies,
  UnmatchedPreviewEventQueue,
  applySourceNavigationLifecycle,
  beginEmbeddedPreviewHandshake,
  completePreviewReadiness,
  disposePreviewCandidate,
  disposePreviewLifecycle,
  handleEditorSelectionLifecycle,
  handleEditorViewportLifecycle,
  handlePreviewEventLifecycle,
  openPreviewLifecycle,
  previewCandidateIdentityIsCurrent,
  previewCandidateIsCurrent,
  previewIsCurrent,
  recreatePreviewsLifecycle,
  reloadPreviewLifecycle,
  requestPreviewCandidate,
} from "./preview-coordinator.mjs";
import type { SourcePosition } from "./protocol.mjs";
import {
  type ExecuteCommandParams,
  type LspMethod,
  type PreviewEvent,
  type PreviewTarget,
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

export {
  findVisibleSourceEditor,
  previewEventAction,
} from "./workspace-selection.mjs";
export {
  executeCreateNote,
  openCommandResult,
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
  readonly #previewEchoes: PreviewEchoState = {};
  readonly #unmatchedPreviewEvents = new UnmatchedPreviewEventQueue();

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
    await openPreviewLifecycle(target, {
      activeEditor: () => vscode.window.activeTextEditor,
      showNoDocument: () => {
        void vscode.window.showInformationMessage(
          vscode.l10n.t("Open a Markdown document first."),
        );
      },
      workspaceFor: (document) =>
        vscode.workspace.getWorkspaceFolder(document.uri),
      start: (workspace) => this.start(workspace),
      sync: (document) => this.syncDocument(document),
      request: (runtime, document, requestedTarget) =>
        this.#requestPreview(runtime, document, requestedTarget),
      candidateCurrent: (candidate) =>
        this.#previewCandidateIsCurrent(candidate),
      candidateIdentityCurrent: (candidate) =>
        this.#previewCandidateIdentityIsCurrent(candidate),
      rejectCandidate: (candidate) => this.#rejectPreviewCandidate(candidate),
      handshake: (owner, preview) => this.#handshakePreview(owner, preview),
      activate: (owner, preview) => this.#activatePreview(owner, preview),
      discardQueued: (origin, previewSessionId) => {
        this.#unmatchedPreviewEvents.takeForDelivery(origin, previewSessionId);
      },
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
      css: defaultPreviewCss,
      log: (message) => this.#log(message),
      previewCurrent: (runtime, preview) =>
        this.#previewIsActive(runtime, preview),
      markReload: (origin, previewSessionId) =>
        this.#markPreviewReload(origin, previewSessionId),
      reload: (runtime, preview) => this.#reloadPreview(runtime, preview),
      report: (error) => this.#report(error),
      dispose: (runtime, preview) => this.#disposePreview(runtime, preview),
      notifyNavigation: (event) => {
        const origin = this.#daemonOrigin();
        origin?.rpc.notify("fleximark/previewEvent", {
          daemonInstanceId: origin.daemonInstanceId,
          ...event,
        });
      },
    });
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
      () =>
        runtime && state
          ? this.#checkpoint(document, state, origin)
          : Promise.resolve(),
      (error) => this.#report(error),
    );
  }

  closeDocument(document: vscode.TextDocument): void {
    const runtime = this.#runtimeForDocument(document);
    coordinateDocumentClose(
      runtime,
      document,
      this.#supervisor.rpc,
      (owner, previewSessionId) => {
        const preview = owner.previews.get(previewSessionId);
        if (preview) void this.#disposePreview(owner, preview);
      },
    );
  }

  selectionChanged(event: vscode.TextEditorSelectionChangeEvent): void {
    handleEditorSelectionLifecycle(event, this.#previewEchoes, {
      runtime: (document) => this.#runtimeForDocument(document),
      currentOrigin: () => this.#daemonOrigin(),
    });
  }

  viewportChanged(event: vscode.TextEditorVisibleRangesChangeEvent): void {
    handleEditorViewportLifecycle(event, this.#previewEchoes, {
      runtime: (document) => this.#runtimeForDocument(document),
      currentOrigin: () => this.#daemonOrigin(),
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
    this.#unmatchedPreviewEvents.clearAll();
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

  #daemonOrigin(): DaemonOrigin | undefined {
    const rpc = this.#supervisor.rpc;
    const daemonInstanceId = this.#supervisor.daemonInstanceId;
    if (!rpc || rpc.closed || !daemonInstanceId) return;
    return {
      rpc,
      daemonInstanceId,
      generation: this.#supervisor.connectionGeneration,
    };
  }

  #originIsCurrent(origin: DaemonOrigin): boolean {
    const current = this.#daemonOrigin();
    return (
      !origin.rpc.closed &&
      current?.rpc === origin.rpc &&
      current.generation === origin.generation &&
      current.daemonInstanceId === origin.daemonInstanceId
    );
  }

  #previewCandidateIsCurrent(candidate: PreviewCandidate): boolean {
    const document = vscode.workspace.textDocuments.find(
      (item) => item.uri.toString() === candidate.documentUri,
    );
    return previewCandidateIsCurrent(candidate, {
      disposed: this.#disposed,
      runtime: this.#runtimes.get(candidate.runtime.workspace.uri.toString()),
      origin: this.#daemonOrigin(),
      documentState: candidate.runtime.documents.get(candidate.documentUri),
      documentVersion: document?.version,
    });
  }

  #previewIsActive(runtime: WorkspaceRuntime, preview: PreviewState): boolean {
    const origin = this.#daemonOrigin();
    return (
      !this.#disposed &&
      this.#runtimes.get(runtime.workspace.uri.toString()) === runtime &&
      previewIsCurrent(runtime, preview, preview.previewSessionId) &&
      origin?.rpc === preview.originRpc &&
      origin.generation === preview.originGeneration &&
      origin.daemonInstanceId === preview.originDaemonInstanceId
    );
  }

  #previewCandidateIdentityIsCurrent(
    candidate: PreviewCandidateOrigin,
  ): boolean {
    const document = vscode.workspace.textDocuments.find(
      (item) => item.uri.toString() === candidate.documentUri,
    );
    return previewCandidateIdentityIsCurrent(candidate, {
      disposed: this.#disposed,
      runtime: this.#runtimes.get(candidate.runtime.workspace.uri.toString()),
      origin: this.#daemonOrigin(),
      documentState: candidate.runtime.documents.get(candidate.documentUri),
      documentVersion: document?.version,
    });
  }

  #previewReadinessDependencies(): PreviewReadinessDependencies {
    return {
      current: (runtime, preview) => this.#previewIsActive(runtime, preview),
      take: (origin, previewSessionId) =>
        this.#unmatchedPreviewEvents.takeForDelivery(origin, previewSessionId),
      markReload: (origin, previewSessionId) =>
        this.#markPreviewReload(origin, previewSessionId),
      deliver: (origin, event) =>
        handlePreviewEventLifecycle(
          origin,
          event,
          this.#runtimes.values(),
          this.#unmatchedPreviewEvents,
          {
            reload: (runtime, preview) =>
              void this.#reloadPreview(runtime, preview),
            overflow: (overflowOrigin) => {
              overflowOrigin.rpc.close();
              this.#unmatchedPreviewEvents.clearOrigin(overflowOrigin);
            },
            navigate: (runtime, preview, navigation) =>
              this.#applySourceNavigation(runtime, preview, navigation),
            reactivate: (runtime, preview) =>
              this.#activatePreview(runtime, preview),
            report: (error) => this.#report(error),
          },
          true,
        ),
      reload: (runtime, preview) => this.#reloadPreview(runtime, preview),
      report: (error) => this.#report(error),
    };
  }

  #markPreviewReload(origin: DaemonOrigin, previewSessionId: string): void {
    const queued = this.#unmatchedPreviewEvents.markReloadRequired(
      origin,
      previewSessionId,
    );
    if (typeof queued !== "object") return;
    for (const affectedOrigin of queued.origins) {
      affectedOrigin.rpc.close();
      this.#unmatchedPreviewEvents.clearOrigin(affectedOrigin);
    }
  }

  #handshakePreview(
    runtime: WorkspaceRuntime,
    preview: PreviewState,
  ): Promise<void> {
    return beginEmbeddedPreviewHandshake(
      runtime,
      preview,
      this.#previewReadinessDependencies(),
    );
  }

  #activatePreview(
    runtime: WorkspaceRuntime,
    preview: PreviewState,
  ): Promise<void> {
    return completePreviewReadiness(
      runtime,
      preview,
      this.#previewReadinessDependencies(),
    );
  }

  async #rejectPreviewCandidate(candidate: PreviewCandidate): Promise<void> {
    this.#unmatchedPreviewEvents.take(
      candidate.origin,
      candidate.result.previewSessionId,
    );
    await disposePreviewCandidate(candidate);
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
    rpc.on("close", () => {
      const daemonInstanceId = this.#supervisor.daemonInstanceId;
      if (daemonInstanceId)
        this.#unmatchedPreviewEvents.clearOrigin({
          rpc,
          generation,
          daemonInstanceId,
        });
      else this.#unmatchedPreviewEvents.clearAll();
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
    this.#unmatchedPreviewEvents.clearAll();
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
    return requestPreviewCandidate(runtime, document, target, {
      currentOrigin: () => this.#daemonOrigin(),
      checkpoint: (openedDocument, state, origin) =>
        this.#checkpoint(openedDocument, state, origin),
      identityCurrent: (candidate) =>
        this.#previewCandidateIdentityIsCurrent(candidate),
      reject: (candidate) => this.#rejectPreviewCandidate(candidate),
    });
  }

  async #checkpoint(
    document: vscode.TextDocument,
    state: DocumentState,
    origin = this.#daemonOrigin(),
  ): Promise<void> {
    await checkpointDocument(document, state, origin);
  }

  async #recreatePreviews(runtime: WorkspaceRuntime): Promise<void> {
    await recreatePreviewsLifecycle(runtime, {
      document: (uri) =>
        vscode.workspace.textDocuments.find(
          (document) => document.uri.toString() === uri,
        ),
      request: (owner, document, target) =>
        this.#requestPreview(owner, document, target),
      currentOrigin: () => this.#daemonOrigin(),
      candidateCurrent: (candidate) =>
        this.#previewCandidateIsCurrent(candidate),
      candidateIdentityCurrent: (candidate) =>
        this.#previewCandidateIdentityIsCurrent(candidate),
      reject: (candidate) => this.#rejectPreviewCandidate(candidate),
      dispose: (owner, preview) => this.#disposePreview(owner, preview),
      handshake: (owner, preview) => this.#handshakePreview(owner, preview),
      activate: (owner, preview) => this.#activatePreview(owner, preview),
      discardQueued: (origin, previewSessionId) => {
        this.#unmatchedPreviewEvents.takeForDelivery(origin, previewSessionId);
      },
      markReload: (origin, previewSessionId) =>
        this.#markPreviewReload(origin, previewSessionId),
      reload: (owner, preview) => this.#reloadPreview(owner, preview),
      openExternal: async (url) => {
        await vscode.env.openExternal(vscode.Uri.parse(url));
      },
      report: (error) => this.#report(error),
    });
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
      const event = message.params as PreviewEvent;
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
        this.#unmatchedPreviewEvents,
        {
          reload: (runtime, preview) =>
            void this.#reloadPreview(runtime, preview),
          overflow: (overflowOrigin) => {
            overflowOrigin.rpc.close();
            this.#unmatchedPreviewEvents.clearOrigin(overflowOrigin);
          },
          navigate: (runtime, preview, navigation) =>
            this.#applySourceNavigation(runtime, preview, navigation),
          reactivate: (runtime, preview) =>
            this.#activatePreview(runtime, preview),
          report: (error) => this.#report(error),
        },
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

  #handleInvalidDaemonMessage(
    message: JsonRpcRequest,
    connection: JsonRpcConnection,
    generation: number,
  ): void {
    if (
      connection.closed ||
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
    const queued = this.#unmatchedPreviewEvents.markReloadRequired(
      {
        rpc: connection,
        generation,
        daemonInstanceId: params.daemonInstanceId as string,
      },
      params.previewSessionId,
    );
    if (typeof queued === "object")
      for (const affectedOrigin of queued.origins) {
        affectedOrigin.rpc.close();
        this.#unmatchedPreviewEvents.clearOrigin(affectedOrigin);
      }
  }

  async #applySourceNavigation(
    runtime: WorkspaceRuntime,
    preview: PreviewState,
    event: SourceNavigationEvent,
  ): Promise<void> {
    await applySourceNavigationLifecycle(runtime, preview, event, {
      document: (uri) =>
        vscode.workspace.textDocuments.find(
          (document) => document.uri.toString() === uri,
        ),
      sourcePositionInDocument: (document, position) =>
        this.#sourcePositionInDocument(document, position),
      range: (document, navigation) => {
        const { start, end } = navigation.sourceRange;
        return new vscode.Range(
          start.line,
          sourcePositionToCharacter(document.lineAt(start.line).text, start),
          end.line,
          sourcePositionToCharacter(document.lineAt(end.line).text, end),
        );
      },
      visibleEditor: (documentUri, sourceViewColumn) =>
        findVisibleSourceEditor(
          vscode.window.visibleTextEditors,
          documentUri,
          sourceViewColumn,
        ),
      showEditor: (document, sourceViewColumn) =>
        vscode.window.showTextDocument(document, {
          viewColumn: sourceViewColumn,
          preserveFocus: true,
          preview: false,
        }),
      current: (owner, item) => this.#previewIsActive(owner, item),
      documentOpen: (document) =>
        vscode.workspace.textDocuments.includes(document),
      selection: (range) => new vscode.Selection(range.start, range.end),
      select: (editor, selection) => {
        editor.selection = selection;
      },
      reveal: (editor, range, kind) =>
        editor.revealRange(
          range,
          kind === "center"
            ? vscode.TextEditorRevealType.InCenterIfOutsideViewport
            : vscode.TextEditorRevealType.AtTop,
        ),
      setSelectionEcho: (uri, value) => {
        const token = { uri, value };
        this.#previewEchoes.selection = token;
        return token;
      },
      clearSelectionEcho: (token) => {
        if (this.#previewEchoes.selection === token)
          this.#previewEchoes.selection = undefined;
      },
      setViewportEcho: (uri) => {
        const token = { uri };
        this.#previewEchoes.viewport = token;
        return token;
      },
      clearViewportEcho: (token) => {
        if (this.#previewEchoes.viewport === token)
          this.#previewEchoes.viewport = undefined;
      },
      schedule: (callback, delay) => void setTimeout(callback, delay),
    });
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
  ): Promise<boolean> {
    return reloadPreviewLifecycle(runtime, preview, {
      disposed: () => this.#disposed,
      currentOrigin: () => this.#daemonOrigin(),
      reportFailure: (message) => this.#appendOutputLine(message),
    });
  }

  async #disposePreview(
    runtime: WorkspaceRuntime,
    preview: PreviewState,
  ): Promise<void> {
    await disposePreviewLifecycle(runtime, preview, {
      clearQueued: (origin, previewSessionId) => {
        this.#unmatchedPreviewEvents.take(origin, previewSessionId);
      },
      reportFailure: (message) => this.#appendOutputLine(message),
    });
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
