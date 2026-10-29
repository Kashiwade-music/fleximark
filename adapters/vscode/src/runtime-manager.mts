import {
  type InitializeResult,
  type LspMethod,
  PROTOCOL_VERSION,
} from "@fleximark/contracts/protocol";
import { type ChildProcessWithoutNullStreams, spawn } from "node:child_process";
import * as path from "node:path";
import * as vscode from "vscode";

import type { AdapterLog } from "./adapter-log.mjs";
import { DaemonConnection } from "./daemon-connection.mjs";
import {
  DaemonSupervisor,
  type DaemonSupervisorDependencies,
} from "./daemon-supervisor.mjs";
import {
  changeDocument,
  closeDocument,
  replayDocumentSessions,
  resetDocumentSessions,
  syncDocument,
} from "./document-coordinator.mjs";
import { initializeEditor } from "./editor-service.mjs";
import {
  nodeReleaseManifestEnvironment,
  verifiedBundledDaemon,
} from "./release-manifest.mjs";
import { JsonRpcConnection, JsonRpcResponseError } from "./rpc.mjs";
import {
  ensureRuntime,
  reconcileRuntimeMembership,
  runtimeKey,
} from "./runtime-membership.mjs";
import type { DocumentRuntime } from "./runtime-state.mjs";

type Supervisor = DaemonSupervisor<
  ChildProcessWithoutNullStreams,
  JsonRpcConnection,
  NodeJS.Timeout,
  vscode.Disposable
>;

export interface RuntimePorts {
  connected(connection: DaemonConnection): void;
  replayPreviews(): Promise<number>;
}

/** Owns process recovery, live workspace membership and document synchronization. */
export class RuntimeManager {
  readonly runtimes = new Map<string, DocumentRuntime>();
  readonly supervisor: Supervisor;
  #connection?: DaemonConnection;
  #initialized?: InitializeResult;
  #workspaceUris = new Set<string>();
  #workspaceSync: Promise<void> = Promise.resolve();
  #disposed = false;

  constructor(
    private readonly context: vscode.ExtensionContext,
    private readonly logger: AdapterLog,
    private readonly ports: RuntimePorts,
  ) {
    const dependencies: DaemonSupervisorDependencies<
      ChildProcessWithoutNullStreams,
      JsonRpcConnection,
      NodeJS.Timeout,
      vscode.Disposable
    > = {
      now: () => Date.now(),
      schedule: (callback, delay) => setTimeout(callback, delay),
      cancel: clearTimeout,
      runtimeCount: () => this.runtimes.size,
      resolveBinary: async () =>
        vscode.workspace
          .getConfiguration("fleximark")
          .get<string>("daemonPath") ||
        verifiedBundledDaemon(
          nodeReleaseManifestEnvironment(context.extensionUri.fsPath),
        ),
      spawn: (binary) =>
        spawn(binary, ["lsp"], {
          cwd: context.extensionUri.fsPath,
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
        new Promise<void>((resolve) => {
          if (child.exitCode !== null || child.signalCode !== null) resolve();
          else child.once("exit", () => resolve());
        }),
      onExit: (child, listener) => child.once("exit", listener),
      onRpcClose: (rpc, listener) => rpc.on("close", listener),
      rpcClosed: (rpc) => rpc.closed,
      closeRpc: (rpc, reason) => {
        if (this.#connection?.rpc === rpc) this.#connection.dispose(reason);
        else rpc.close(reason);
      },
      kill: (child) => {
        child.kill();
      },
      bindConnection: (child, rpc, generation) => {
        this.#connection?.dispose();
        const connection = new DaemonConnection(rpc, generation);
        this.#connection = connection;
        child.stderr.setEncoding("utf8");
        child.stderr.on("data", (value: string) => logger.stderr(value));
        ports.connected(connection);
      },
      initializeProtocol: async (rpc) => {
        const folders = vscode.workspace.workspaceFolders ?? [];
        this.#initialized = await initializeEditor(
          rpc,
          context.extension.packageJSON.version,
          folders,
        );
        this.#workspaceUris = new Set(
          folders.map((folder) => folder.uri.toString()),
        );
      },
      initializeWorkspaces: async () => {
        const initialized = this.#initialized;
        if (!initialized || initialized.protocolVersion !== PROTOCOL_VERSION)
          throw new Error(
            vscode.l10n.t("Invalid FlexiMark initialization response"),
          );
        this.#connection?.identify(initialized.daemonInstanceId);
        this.#initialized = undefined;
        for (const status of initialized.workspaceStatuses)
          if (!status.enabled)
            logger.write(
              `FlexiMark workspace disabled: ${status.error ?? "invalid configuration"}`,
              "error",
            );
        return initialized.daemonInstanceId;
      },
      replayDocuments: async () => {
        await replayDocumentSessions(
          vscode.workspace.textDocuments.filter((document) =>
            this.runtimeForDocument(document)?.documents.has(
              document.uri.toString(),
            ),
          ),
          (document) => this.syncKnownDocument(document),
          (document, error) => {
            this.runtimeForDocument(document)?.documents.delete(
              document.uri.toString(),
            );
            logger.report(error);
          },
        );
      },
      replayPreviews: async () => ({
        documents: [...this.runtimes.values()].reduce(
          (count, runtime) => count + runtime.documents.size,
          0,
        ),
        previews: await ports.replayPreviews(),
      }),
      resetAdapterState: () => {
        this.#connection?.dispose();
        resetDocumentSessions(this.runtimes.values());
      },
      requestShutdown: (rpc) => rpc.requestLsp("shutdown", undefined, 1_000),
      notifyExit: (rpc) => rpc.notifyLsp("exit"),
      log: (message) => logger.write(message, "debug"),
      report: (error) => logger.report(error),
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
        return choice === vscode.l10n.t("Retry")
          ? "retry"
          : choice === vscode.l10n.t("Open Output")
            ? "openOutput"
            : undefined;
      },
      openOutput: () => logger.output.show(true),
      disposeStatus: (status) => status.dispose(),
    };
    this.supervisor = new DaemonSupervisor(dependencies);
  }

  get connection(): DaemonConnection | undefined {
    const connection = this.#connection;
    return connection &&
      !connection.signal.aborted &&
      connection.daemonInstanceId
      ? connection
      : undefined;
  }

  report(error: unknown): void {
    this.logger.report(error);
  }
  log(message: string): void {
    this.logger.write(message, "debug");
  }

  async start(
    workspace?: vscode.WorkspaceFolder,
  ): Promise<DocumentRuntime | undefined> {
    if (this.#disposed) return;
    reconcileRuntimeMembership(this.runtimes, (uri) =>
      vscode.workspace.getWorkspaceFolder(vscode.Uri.parse(uri)),
    );
    const runtime = ensureRuntime(this.runtimes, workspace);
    await this.supervisor.ensure();
    if (this.#disposed) return;
    await this.synchronizeWorkspaces();
    return runtime;
  }

  private synchronizeWorkspaces(): Promise<void> {
    const operation = async () => {
      const connection = this.connection;
      if (!connection) return;
      const folders = vscode.workspace.workspaceFolders ?? [];
      const uris = new Set(folders.map((folder) => folder.uri.toString()));
      const added = folders.filter(
        (folder) => !this.#workspaceUris.has(folder.uri.toString()),
      );
      const removed = [...this.#workspaceUris].filter((uri) => !uris.has(uri));
      if (added.length || removed.length)
        connection.rpc.notifyLsp("workspace/didChangeWorkspaceFolders", {
          event: {
            added: added.map((folder) => ({
              uri: folder.uri.toString(),
              name: folder.name,
            })),
            removed: removed.map((uri) => ({ uri, name: path.basename(uri) })),
          },
        });
      this.#workspaceUris = uris;
      for (const folder of added) {
        if (connection.signal.aborted) return;
        await connection.rpc.request("fleximark/reconfigureWorkspace", {
          daemonInstanceId: connection.daemonInstanceId,
          workspaceUri: folder.uri.toString(),
          trusted: vscode.workspace.isTrusted,
        });
      }
    };
    const current = this.#workspaceSync.then(operation);
    this.#workspaceSync = current.catch(() => undefined);
    return current;
  }

  runtimeForDocument(
    document: vscode.TextDocument,
  ): DocumentRuntime | undefined {
    const workspace = vscode.workspace.getWorkspaceFolder(document.uri);
    return (
      this.runtimes.get(runtimeKey(workspace)) ??
      [...this.runtimes.values()].find((runtime) =>
        runtime.documents.has(document.uri.toString()),
      )
    );
  }

  async activateDocument(document?: vscode.TextDocument): Promise<void> {
    if (document?.languageId !== "markdown" || this.#disposed) return;
    await this.start(vscode.workspace.getWorkspaceFolder(document.uri));
    await this.syncKnownDocument(document);
  }

  async syncDocument(document: vscode.TextDocument): Promise<void> {
    await this.activateDocument(document);
  }

  private async syncKnownDocument(
    document: vscode.TextDocument,
  ): Promise<void> {
    const runtime = this.runtimeForDocument(document);
    const connection = this.connection;
    if (!runtime || !connection || this.#disposed) return;
    await syncDocument(runtime, document, connection, (changed) =>
      this.changeDocument(changed),
    );
  }

  changeDocument(document: vscode.TextDocument): void {
    changeDocument(
      this.runtimeForDocument(document),
      document,
      this.connection?.rpc,
      () => this.syncDocument(document),
      (error) => this.logger.report(error),
    );
  }

  closeDocument(document: vscode.TextDocument): void {
    closeDocument(
      this.runtimeForDocument(document),
      document,
      this.connection?.rpc,
    );
  }

  async requestLanguageFeature<T>(
    method: LspMethod,
    document: vscode.TextDocument,
    params: object = {},
  ): Promise<T | undefined> {
    if (document.languageId !== "markdown") return;
    await this.activateDocument(document);
    const connection = this.connection;
    if (!connection || this.#disposed) return;
    try {
      const result = await connection.rpc.requestLsp<T>(method, {
        textDocument: { uri: document.uri.toString() },
        ...params,
      });
      return connection.signal.aborted ? undefined : result;
    } catch (error) {
      if (
        connection.signal.aborted ||
        (error instanceof JsonRpcResponseError &&
          error.code === -32602 &&
          error.message.startsWith("document is not open"))
      )
        return;
      throw error;
    }
  }

  async reconfigureWorkspace(workspace: vscode.WorkspaceFolder): Promise<void> {
    await this.start(workspace);
    const connection = this.connection;
    if (connection)
      await connection.rpc.request("fleximark/reconfigureWorkspace", {
        daemonInstanceId: connection.daemonInstanceId,
        workspaceUri: workspace.uri.toString(),
        trusted: vscode.workspace.isTrusted,
      });
  }

  removeWorkspace(workspace: vscode.WorkspaceFolder): void {
    reconcileRuntimeMembership(
      this.runtimes,
      (uri) => vscode.workspace.getWorkspaceFolder(vscode.Uri.parse(uri)),
      runtimeKey(workspace),
    );
    void this.synchronizeWorkspaces().catch((error: unknown) =>
      this.logger.report(error),
    );
  }

  dispose(): void {
    if (this.#disposed) return;
    this.#disposed = true;
    this.#connection?.dispose();
    this.supervisor.dispose();
    this.runtimes.clear();
  }
}
