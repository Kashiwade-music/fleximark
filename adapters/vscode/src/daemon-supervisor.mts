import { errorMessage } from "./error-policy.mjs";

export interface DaemonSupervisorDependencies<Process, Rpc, Timer, Status> {
  now(): number;
  schedule(callback: () => void, delay: number): Timer;
  cancel(timer: Timer): void;
  workspaceCount(): number;
  resolveBinary(): Promise<string>;
  spawn(binary: string): Process;
  createRpc(process: Process): Rpc;
  waitForSpawn(process: Process): Promise<void>;
  waitForExit(process: Process): Promise<void>;
  onExit(
    process: Process,
    listener: (code: number | null, signal: string | null) => void,
  ): void;
  onRpcClose(rpc: Rpc, listener: () => void): void;
  rpcClosed(rpc: Rpc): boolean;
  closeRpc(rpc: Rpc, reason?: Error): void;
  kill(process: Process): void;
  bindConnection(process: Process, rpc: Rpc, generation: number): void;
  initializeProtocol(rpc: Rpc): Promise<void>;
  initializeWorkspaces(rpc: Rpc): Promise<string>;
  replayDocuments(rpc: Rpc): Promise<void>;
  replayPreviews(rpc: Rpc): Promise<{ documents: number; previews: number }>;
  resetAdapterState(): void;
  requestShutdown(rpc: Rpc): Promise<void>;
  notifyExit(rpc: Rpc): void;
  log(message: string): void;
  report(error: unknown): void;
  showRecoveryStatus(): Status;
  showRecoveredStatus(): Status;
  showRepeatedFailure(): Promise<"retry" | "openOutput" | undefined>;
  openOutput(): void;
  disposeStatus(status: Status): void;
}

export interface DaemonSupervisorSnapshot<Process, Rpc> {
  process?: Process;
  rpc?: Rpc;
  daemonInstanceId?: string;
  connectionGeneration: number;
  workspaceRevision: number;
  appliedWorkspaceRevision: number;
  restartCount: number;
  restarting: boolean;
}

export class DaemonSupervisor<Process, Rpc, Timer, Status> {
  readonly #dependencies: DaemonSupervisorDependencies<
    Process,
    Rpc,
    Timer,
    Status
  >;
  #process?: Process;
  #rpc?: Rpc;
  #daemonInstanceId?: string;
  #startPromise?: Promise<void>;
  #restartTimer?: Timer;
  #restartCount = 0;
  #lastStartedAt = 0;
  #connectionGeneration = 0;
  #workspaceRevision = 0;
  #appliedWorkspaceRevision = -1;
  #restarting = false;
  #recoverySequence = 0;
  #recoveryId?: string;
  #recoveryStatus?: Status;
  #stopping = false;
  #disposed = false;

  constructor(
    dependencies: DaemonSupervisorDependencies<Process, Rpc, Timer, Status>,
  ) {
    this.#dependencies = dependencies;
  }

  get process(): Process | undefined {
    return this.#process;
  }

  get rpc(): Rpc | undefined {
    return this.#rpc;
  }

  get daemonInstanceId(): string | undefined {
    return this.#daemonInstanceId;
  }

  get connectionGeneration(): number {
    return this.#connectionGeneration;
  }

  snapshot(): DaemonSupervisorSnapshot<Process, Rpc> {
    return {
      process: this.#process,
      rpc: this.#rpc,
      daemonInstanceId: this.#daemonInstanceId,
      connectionGeneration: this.#connectionGeneration,
      workspaceRevision: this.#workspaceRevision,
      appliedWorkspaceRevision: this.#appliedWorkspaceRevision,
      restartCount: this.#restartCount,
      restarting: this.#restarting,
    };
  }

  workspaceChanged(): void {
    this.#workspaceRevision += 1;
  }

  beginDispose(): void {
    this.#stopping = true;
  }

  async ensure(): Promise<void> {
    if (this.#stopping || this.#disposed) return;
    if (this.#restartTimer !== undefined) {
      this.#dependencies.cancel(this.#restartTimer);
      this.#restartTimer = undefined;
    }
    while (this.#dependencies.workspaceCount()) {
      if (this.#stopping || this.#disposed) return;
      if (this.#restartTimer !== undefined) {
        this.#dependencies.cancel(this.#restartTimer);
        this.#restartTimer = undefined;
      }
      const pending = this.#startPromise;
      if (pending) {
        await pending;
        if (this.#stopping || this.#disposed) return;
        continue;
      }
      if (
        this.#rpc &&
        !this.#dependencies.rpcClosed(this.#rpc) &&
        this.#daemonInstanceId &&
        this.#appliedWorkspaceRevision === this.#workspaceRevision
      )
        return;

      const operation = (
        this.#process ? this.#restart() : this.#launch()
      ).catch((error: unknown) => {
        const rpc = this.#rpc;
        const process = this.#process;
        this.#rpc = undefined;
        this.#process = undefined;
        this.#daemonInstanceId = undefined;
        if (rpc)
          this.#dependencies.closeRpc(
            rpc,
            error instanceof Error ? error : new Error(String(error)),
          );
        if (process) this.#dependencies.kill(process);
        if (
          this.#recoveryId &&
          !this.#stopping &&
          this.#dependencies.workspaceCount()
        )
          this.#scheduleRecovery(`launch failed: ${errorMessage(error)}`);
        throw error;
      });
      this.#startPromise = operation;
      try {
        await operation;
      } finally {
        if (this.#startPromise === operation) this.#startPromise = undefined;
      }
      if (this.#stopping || this.#disposed) return;
    }
  }

  stop(): void {
    if (this.#restartTimer !== undefined) {
      this.#dependencies.cancel(this.#restartTimer);
      this.#restartTimer = undefined;
    }
    const rpc = this.#rpc;
    const process = this.#process;
    this.#rpc = undefined;
    this.#process = undefined;
    this.#daemonInstanceId = undefined;
    if (rpc) {
      void this.#dependencies
        .requestShutdown(rpc)
        .catch(() => undefined)
        .finally(() => {
          this.#dependencies.notifyExit(rpc);
          this.#dependencies.closeRpc(rpc);
          if (process) this.#dependencies.kill(process);
        });
    } else if (process) {
      this.#dependencies.kill(process);
    }
  }

  dispose(): void {
    if (this.#disposed) return;
    this.#disposed = true;
    this.#stopping = true;
    this.stop();
    this.#disposeRecoveryStatus();
  }

  async #restart(): Promise<void> {
    const process = this.#process;
    if (!process) return this.#launch();
    this.#restarting = true;
    const exited = this.#dependencies.waitForExit(process);
    if (this.#rpc)
      this.#dependencies.closeRpc(
        this.#rpc,
        new Error("FlexiMark daemon is restarting"),
      );
    this.#dependencies.kill(process);
    await exited;
    if (
      this.#stopping ||
      this.#disposed ||
      !this.#dependencies.workspaceCount()
    ) {
      this.#restarting = false;
      return;
    }
    this.#rpc = undefined;
    this.#process = undefined;
    this.#daemonInstanceId = undefined;
    this.#restarting = false;
    await this.#launch();
  }

  async #launch(): Promise<void> {
    const binary = await this.#dependencies.resolveBinary();
    if (
      this.#stopping ||
      this.#disposed ||
      !this.#dependencies.workspaceCount()
    )
      return;
    const process = this.#dependencies.spawn(binary);
    this.#process = process;
    const rpc = this.#dependencies.createRpc(process);
    const generation = ++this.#connectionGeneration;
    const recoveryId =
      this.#recoveryId ?? `launch-${this.#recoverySequence + 1}`;
    this.#dependencies.log(
      `[${recoveryId}] launching daemon generation=${generation} workspaces=${this.#dependencies.workspaceCount()} workspaceRevision=${this.#workspaceRevision}`,
    );
    this.#rpc = rpc;
    this.#dependencies.bindConnection(process, rpc, generation);
    this.#dependencies.onRpcClose(rpc, () => {
      if (
        this.#rpc === rpc &&
        this.#process === process &&
        !this.#stopping &&
        !this.#restarting
      )
        this.#dependencies.kill(process);
    });
    this.#dependencies.onExit(process, (code, signal) =>
      this.#daemonExited(process, code, signal),
    );
    await this.#dependencies.waitForSpawn(process);
    if (this.#process !== process || this.#rpc !== rpc || this.#stopping)
      return;
    await this.#dependencies.initializeProtocol(rpc);
    if (this.#process !== process || this.#rpc !== rpc || this.#stopping)
      return;
    const workspaceRevision = this.#workspaceRevision;
    const daemonInstanceId = await this.#dependencies.initializeWorkspaces(rpc);
    if (this.#process !== process || this.#rpc !== rpc || this.#stopping)
      return;
    this.#daemonInstanceId = daemonInstanceId;
    await this.#dependencies.replayDocuments(rpc);
    if (this.#process !== process || this.#rpc !== rpc || this.#stopping)
      return;
    this.#appliedWorkspaceRevision = workspaceRevision;
    this.#lastStartedAt = this.#dependencies.now();
    const replayed = await this.#dependencies.replayPreviews(rpc);
    if (
      this.#process !== process ||
      this.#rpc !== rpc ||
      this.#stopping ||
      this.#disposed
    )
      return;
    this.#dependencies.log(
      `[${recoveryId}] daemon ready generation=${generation}; replayed documents=${replayed.documents} previews=${replayed.previews}`,
    );
    if (this.#recoveryId) {
      this.#recoveryId = undefined;
      this.#disposeRecoveryStatus();
      this.#recoveryStatus = this.#dependencies.showRecoveredStatus();
    }
  }

  #daemonExited(
    process: Process,
    code: number | null,
    signal: string | null,
  ): void {
    if (this.#process !== process || this.#stopping) return;
    if (this.#rpc)
      this.#dependencies.closeRpc(
        this.#rpc,
        new Error("FlexiMark daemon exited"),
      );
    this.#rpc = undefined;
    this.#process = undefined;
    this.#daemonInstanceId = undefined;
    this.#dependencies.resetAdapterState();
    if (this.#restarting) return;
    if (this.#dependencies.now() - this.#lastStartedAt > 30_000)
      this.#restartCount = 0;
    const recoveryId = `recovery-${++this.#recoverySequence}`;
    this.#recoveryId = recoveryId;
    this.#scheduleRecovery(
      `daemon exited code=${code ?? "none"} signal=${signal ?? "none"}`,
    );
  }

  #scheduleRecovery(reason: string): void {
    const recoveryId =
      this.#recoveryId ?? `recovery-${++this.#recoverySequence}`;
    this.#recoveryId = recoveryId;
    this.#restartCount += 1;
    this.#dependencies.log(
      `[${recoveryId}] ${reason}; attempt=${this.#restartCount}`,
    );
    this.#disposeRecoveryStatus();
    this.#recoveryStatus = this.#dependencies.showRecoveryStatus();
    if (this.#restartCount > 5) {
      this.#disposeRecoveryStatus();
      const generation = this.#connectionGeneration;
      void this.#dependencies
        .showRepeatedFailure()
        .then((choice) => {
          if (
            this.#stopping ||
            this.#disposed ||
            this.#recoveryId !== recoveryId ||
            this.#connectionGeneration !== generation
          )
            return;
          if (choice === "openOutput") {
            this.#dependencies.openOutput();
            return;
          }
          if (choice === "retry") {
            this.#restartCount = 0;
            void this.ensure().catch((error: unknown) =>
              this.#dependencies.report(error),
            );
          }
        })
        .catch((error: unknown) => {
          if (
            !this.#stopping &&
            !this.#disposed &&
            this.#recoveryId === recoveryId &&
            this.#connectionGeneration === generation
          )
            this.#dependencies.report(error);
        });
      return;
    }
    const delay = Math.min(250 * 2 ** (this.#restartCount - 1), 4_000);
    this.#dependencies.log(`[${recoveryId}] retry scheduled in ${delay}ms`);
    if (this.#restartTimer !== undefined)
      this.#dependencies.cancel(this.#restartTimer);
    this.#restartTimer = this.#dependencies.schedule(() => {
      this.#restartTimer = undefined;
      void this.ensure().catch((error: unknown) => {
        this.#dependencies.log(
          `[${recoveryId}] retry failed: ${errorMessage(error)}`,
        );
      });
    }, delay);
  }

  #disposeRecoveryStatus(): void {
    if (this.#recoveryStatus === undefined) return;
    this.#dependencies.disposeStatus(this.#recoveryStatus);
    this.#recoveryStatus = undefined;
  }
}
