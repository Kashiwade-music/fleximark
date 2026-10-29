import type {
  CreatePreviewResult,
  PreviewChangedParams,
  PreviewTarget,
  ServerPreviewEventParams,
} from "@fleximark/contracts/protocol";
import { shouldForwardPreviewNavigation } from "@fleximark/contracts/protocol";
import defaultPreviewCss from "@fleximark/preview/fleximark.css";
import katexCss from "@fleximark/preview/katex.css";
import { PreviewSession } from "@fleximark/preview/session";
import { randomBytes } from "node:crypto";
import * as path from "node:path";
import * as vscode from "vscode";

import { isWebviewInboundMessage } from "../webview/bridge.mjs";
import type { DaemonConnection } from "./daemon-connection.mjs";
import {
  type PreviewEchoState,
  embeddedPreviewShell,
} from "./preview-coordinator.mjs";
import {
  type DocumentRuntime,
  EmbeddedBinding,
  type EmbeddedPreview,
  PreviewBinding,
  type PreviewState,
} from "./runtime-state.mjs";
import { applySourceNavigation } from "./source-navigation.mjs";

export interface PreviewEnvironment {
  readonly runtimes: Map<string, DocumentRuntime>;
  readonly connection: DaemonConnection | undefined;
  start(
    workspace?: vscode.WorkspaceFolder,
  ): Promise<DocumentRuntime | undefined>;
  syncDocument(document: vscode.TextDocument): Promise<void>;
  report(error: unknown): void;
  log(message: string): void;
}

/** Owns preview leases, the only binding exchange, and remote preview disposal. */
export class PreviewController {
  readonly echoes: PreviewEchoState = {};
  readonly #retired = new WeakSet<PreviewBinding>();

  constructor(
    private readonly environment: PreviewEnvironment,
    private readonly extensionUri: vscode.Uri,
  ) {}

  owner(preview: PreviewState): DocumentRuntime | undefined {
    return [...this.environment.runtimes.values()].find(
      (runtime) =>
        runtime.previews.get(preview.binding.previewSessionId)?.binding ===
        preview.binding,
    );
  }

  async open(target: PreviewTarget): Promise<string | undefined> {
    const editor = vscode.window.activeTextEditor;
    if (editor?.document.languageId !== "markdown") {
      void vscode.window.showInformationMessage(
        vscode.l10n.t("Open a Markdown document first."),
      );
      return;
    }
    const runtime = await this.environment.start(
      vscode.workspace.getWorkspaceFolder(editor.document.uri),
    );
    if (!runtime) return;
    await this.environment.syncDocument(editor.document);
    const preview = await this.request(runtime, editor.document, target);
    if (!preview || !preview.binding.isCurrent()) return;
    preview.sourceViewColumn = editor.viewColumn;
    if (preview.target === "externalBrowser") {
      try {
        await vscode.env.openExternal(vscode.Uri.parse(preview.url));
      } catch (error) {
        await this.release(preview);
        throw error;
      }
      return preview.url;
    }
    try {
      const panel = vscode.window.createWebviewPanel(
        "fleximark.preview",
        `FlexiMark: ${path.basename(editor.document.fileName)}`,
        vscode.workspace
          .getConfiguration("fleximark")
          .get<string>("previewColumn", "beside") === "active"
          ? vscode.ViewColumn.Active
          : vscode.ViewColumn.Beside,
        { enableScripts: true, retainContextWhenHidden: true },
      );
      preview.panel = panel;
      preview.messageToken = randomBytes(32).toString("base64url");
      panel.onDidDispose(() => {
        void this.release(preview);
      });
      panel.webview.onDidReceiveMessage((event: unknown) => {
        if (!isWebviewInboundMessage(event) || !preview.binding.isCurrent())
          return;
        if (event.type === "ready" || event.type === "requestFrame") {
          preview.webviewReady = true;
          void this.deliver(preview, true);
        } else if (
          shouldForwardPreviewNavigation(
            event,
            preview.binding.previewSessionId,
            preview.binding.renderRevision,
          )
        ) {
          preview.binding.connection.rpc.notify("fleximark/previewEvent", {
            daemonInstanceId: preview.binding.connection.daemonInstanceId,
            previewSessionId: event.previewSessionId,
            renderRevision: event.renderRevision,
            event,
          });
        }
      });
      panel.webview.html = embeddedPreviewShell(
        randomBytes(16).toString("base64"),
        preview.messageToken,
        panel.webview.cspSource,
        panel.webview
          .asWebviewUri(
            vscode.Uri.joinPath(
              this.extensionUri,
              "dist",
              "webview",
              "vscode-host.js",
            ),
          )
          .toString(),
        `${katexCss}\n${defaultPreviewCss}`,
      );
      this.environment.log("embedded preview created");
    } catch (error) {
      await this.release(preview);
      throw error;
    }
  }

  async request(
    runtime: DocumentRuntime,
    document: vscode.TextDocument,
    target: PreviewTarget,
  ): Promise<PreviewState | undefined> {
    for (let attempt = 0; attempt < 3; attempt += 1) {
      const connection = this.environment.connection;
      const documentState = runtime.documents.get(document.uri.toString());
      if (!connection || connection.signal.aborted || !documentState?.sessionId)
        return;
      const version = document.version;
      if (documentState.version !== version) continue;
      const documentSessionId = documentState.sessionId;
      let preview: PreviewState | undefined;
      let committed = false;
      const register = (result: CreatePreviewResult) => {
        const current = () => {
          if (!preview) return false;
          const owner = this.owner(preview);
          return (
            !!owner &&
            !owner.removed &&
            owner.documents.get(preview.documentUri) === documentState &&
            documentState.sessionId === documentSessionId &&
            (committed ||
              (document.version === version &&
                documentState.version === version))
          );
        };
        preview =
          target === "embeddedHtml"
            ? {
                target,
                documentUri: document.uri.toString(),
                binding: new EmbeddedBinding(connection, result.frame, current),
              }
            : {
                target,
                documentUri: document.uri.toString(),
                url: result.url ?? "",
                binding: new PreviewBinding(
                  connection,
                  result.previewSessionId,
                  current,
                ),
              };
        if (preview.target === "externalBrowser")
          preview.binding.renderRevision = result.frame.renderRevision;
        this.exchange(undefined, preview, runtime);
      };
      const result = await connection.rpc.request(
        "fleximark/createPreview",
        {
          daemonInstanceId: connection.daemonInstanceId,
          documentSessionId,
          expectedDocumentVersion: version,
          target,
        },
        undefined,
        register,
      );
      if (!preview) return;
      if (!preview.binding.isCurrent()) {
        await this.release(preview);
        if (connection.signal.aborted) return;
        continue;
      }
      committed = true;
      if (target === "externalBrowser") {
        if (!result.url) {
          await this.release(preview);
          throw new Error(
            vscode.l10n.t("Daemon omitted the external preview URL"),
          );
        }
      }
      return preview;
    }
  }

  exchange(
    previous: PreviewState | undefined,
    next: PreviewState,
    runtime: DocumentRuntime,
  ): PreviewState {
    if (!previous) {
      runtime.previews.set(next.binding.previewSessionId, next);
      return next;
    }
    const old = previous.binding;
    this.owner(previous)?.previews.delete(old.previewSessionId);
    if (previous.target === "embeddedHtml" && next.target === "embeddedHtml") {
      old.active = false;
      if (old instanceof EmbeddedBinding) old.delivery?.dispose();
      previous.binding = next.binding;
      previous.documentUri = next.documentUri;
      if (previous.panel)
        previous.panel.title = `FlexiMark: ${path.basename(next.documentUri)}`;
      runtime.previews.set(next.binding.previewSessionId, previous);
      void this.retire(old);
      return previous;
    }
    throw new Error("Preview target cannot change during binding exchange");
  }

  async release(preview: PreviewState): Promise<void> {
    const binding = preview.binding;
    this.owner(preview)?.previews.delete(binding.previewSessionId);
    binding.active = false;
    if (preview.target === "embeddedHtml") {
      preview.binding.delivery?.dispose();
      preview.panel?.dispose();
    }
    await this.retire(binding);
  }

  async retire(binding: PreviewBinding): Promise<void> {
    if (this.#retired.has(binding)) return;
    this.#retired.add(binding);
    if (binding.connection.signal.aborted || binding.connection.rpc.closed)
      return;
    try {
      await binding.connection.rpc.request("fleximark/disposePreview", {
        daemonInstanceId: binding.connection.daemonInstanceId,
        previewSessionId: binding.previewSessionId,
      });
    } catch (error) {
      if (!binding.connection.signal.aborted) this.environment.report(error);
    }
  }

  async follow(editor?: vscode.TextEditor): Promise<void> {
    if (
      editor?.document.languageId !== "markdown" ||
      editor.viewColumn === undefined
    )
      return;
    for (const runtime of [...this.environment.runtimes.values()])
      for (const preview of [...runtime.previews.values()]) {
        if (
          preview.target !== "embeddedHtml" ||
          !preview.panel ||
          preview.sourceViewColumn !== editor.viewColumn ||
          (preview.documentUri === editor.document.uri.toString() &&
            preview.binding.isCurrent())
        )
          continue;
        const destination = await this.environment.start(
          vscode.workspace.getWorkspaceFolder(editor.document.uri),
        );
        if (!destination) continue;
        await this.environment.syncDocument(editor.document);
        const next = await this.request(
          destination,
          editor.document,
          "embeddedHtml",
        );
        if (!next) continue;
        if (
          vscode.window.activeTextEditor?.document !== editor.document ||
          !next.binding.isCurrent()
        ) {
          await this.release(next);
          continue;
        }
        const current = this.exchange(preview, next, destination);
        if (current.target === "embeddedHtml")
          await this.deliver(current, true);
      }
  }

  async replay(): Promise<number> {
    let count = 0;
    for (const runtime of [...this.environment.runtimes.values()])
      for (const preview of [...runtime.previews.values()]) {
        const document = vscode.workspace.textDocuments.find(
          (document) => document.uri.toString() === preview.documentUri,
        );
        if (!document || preview.binding.isCurrent()) continue;
        const next = await this.request(runtime, document, preview.target);
        if (!next) continue;
        if (
          preview.target === "externalBrowser" &&
          next.target === "externalBrowser"
        ) {
          await vscode.env.openExternal(vscode.Uri.parse(next.url));
          await this.release(preview);
          count += 1;
          continue;
        }
        const current = this.exchange(preview, next, runtime);
        if (current.target === "embeddedHtml")
          await this.deliver(current, true);
        count += 1;
      }
    return count;
  }

  async rerender(preview: PreviewState): Promise<boolean> {
    const binding = preview.binding;
    if (!binding.isCurrent()) return false;
    try {
      await binding.connection.rpc.request("fleximark/rerenderPreview", {
        daemonInstanceId: binding.connection.daemonInstanceId,
        previewSessionId: binding.previewSessionId,
      });
    } catch (error) {
      if (binding.isCurrent()) throw error;
      return false;
    }
    return binding.isCurrent() && preview.binding === binding;
  }

  async refresh(): Promise<void> {
    await Promise.all(
      [...this.environment.runtimes.values()].flatMap((runtime) =>
        [...runtime.previews.values()].map(async (preview) => {
          if (
            (await this.rerender(preview)) &&
            preview.target === "embeddedHtml"
          )
            await this.deliver(preview, true);
        }),
      ),
    );
  }

  frame(connection: DaemonConnection, changed: PreviewChangedParams): void {
    const preview = [...this.environment.runtimes.values()]
      .map((runtime) => runtime.previews.get(changed.previewSessionId))
      .find((preview) => preview !== undefined);
    if (
      !preview ||
      !preview.binding.isCurrent(connection) ||
      changed.daemonInstanceId !== connection.daemonInstanceId ||
      changed.frame.previewSessionId !== preview.binding.previewSessionId ||
      changed.frame.renderRevision !== changed.renderRevision
    )
      return;
    if (preview.target === "externalBrowser") {
      preview.binding.renderRevision = Math.max(
        preview.binding.renderRevision,
        changed.renderRevision,
      );
      return;
    }
    if (changed.renderRevision <= preview.binding.frame.renderRevision) return;
    preview.binding.frame = changed.frame;
    if (preview.webviewReady) void this.deliver(preview);
  }

  async event(
    connection: DaemonConnection,
    event: ServerPreviewEventParams,
  ): Promise<void> {
    const preview = [...this.environment.runtimes.values()]
      .map((runtime) => runtime.previews.get(event.previewSessionId))
      .find((preview) => preview !== undefined);
    if (
      !preview ||
      !preview.binding.isCurrent(connection) ||
      event.daemonInstanceId !== connection.daemonInstanceId
    )
      return;
    if (preview.target === "externalBrowser") {
      if (event.renderRevision < preview.binding.renderRevision) return;
      preview.binding.renderRevision = event.renderRevision;
    }
    if (
      event.event.type === "selectSource" ||
      event.event.type === "revealSource"
    ) {
      if (event.renderRevision !== preview.binding.renderRevision) return;
      await applySourceNavigation(preview, event.event, this.echoes);
    } else if (preview.target === "embeddedHtml")
      await this.delivery(preview).receiveNavigation(event.event);
  }

  private delivery(preview: EmbeddedPreview): PreviewSession {
    const binding = preview.binding;
    if (binding.delivery) return binding.delivery;
    const session = new PreviewSession({
      apply: async (frame) => {
        if (
          !binding.isCurrent() ||
          preview.binding !== binding ||
          binding.delivery !== session ||
          !preview.panel ||
          !preview.webviewReady
        )
          return false;
        const accepted = await preview.panel.webview.postMessage({
          type: "previewFrame",
          messageToken: preview.messageToken,
          frame,
        });
        if (
          !binding.isCurrent() ||
          preview.binding !== binding ||
          binding.delivery !== session
        )
          return false;
        if (accepted)
          binding.renderRevision = Math.max(
            binding.renderRevision,
            frame.renderRevision,
          );
        return accepted;
      },
      navigate: async (event) => {
        if (
          binding.isCurrent() &&
          preview.binding === binding &&
          binding.delivery === session
        )
          await preview.panel?.webview.postMessage({
            type: "previewEvent",
            messageToken: preview.messageToken,
            event,
          });
      },
      requestRecovery: () => {
        void this.rerender(preview).catch((error: unknown) =>
          this.environment.report(error),
        );
      },
      reportError: (error) => this.environment.report(error),
    });
    binding.delivery = session;
    return session;
  }

  async deliver(preview: EmbeddedPreview, force = false): Promise<void> {
    if (!preview.binding.isCurrent() || !preview.webviewReady) return;
    if (force) {
      preview.binding.delivery?.dispose();
      preview.binding.delivery = undefined;
    }
    await this.delivery(preview).receiveFrame(preview.binding.frame);
  }

  dispose(): void {
    for (const runtime of this.environment.runtimes.values())
      for (const preview of [...runtime.previews.values()])
        void this.release(preview);
  }
}
