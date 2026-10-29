import {
  DaemonSupervisor,
  type DaemonSupervisorDependencies,
} from "fleximark/src/daemon-supervisor";
import { errorMessage } from "fleximark/src/error-policy";
import * as assert from "node:assert/strict";

interface FakeProcess {
  id: number;
  exited: boolean;
  killed: boolean;
  exitListeners: ((code: number | null, signal: string | null) => void)[];
  exitHistory: ((code: number | null, signal: string | null) => void)[];
  exitWaiters: (() => void)[];
}

interface FakeRpc {
  id: number;
  closed: boolean;
  closeListeners: (() => void)[];
}

interface FakeTimer {
  callback: () => void;
  delay: number;
  cancelled: boolean;
}

interface FakeStatus {
  kind: "recovering" | "recovered";
  disposed: boolean;
}

class SupervisorHarness {
  now = 0;
  runtimeCount = 1;
  spawnCount = 0;
  shutdownCount = 0;
  resetCount = 0;
  repeatedFailureCount = 0;
  openOutputCount = 0;
  repeatedFailureChoice?: "retry" | "openOutput";
  autoExitOnKill = true;
  rejectShutdown = false;
  failSpawnIds = new Set<number>();
  logs: string[] = [];
  lifecycle: string[] = [];
  reports: unknown[] = [];
  timers: FakeTimer[] = [];
  processes: FakeProcess[] = [];
  statuses: FakeStatus[] = [];
  replayDocumentsHook?: () => void;
  replayPreviewsHook?: (process: FakeProcess) => void | Promise<void>;
  resolveBinaryHook?: () => Promise<string>;
  repeatedFailureHook?: () => Promise<"retry" | "openOutput" | undefined>;
  supervisor: DaemonSupervisor<FakeProcess, FakeRpc, FakeTimer, FakeStatus>;

  constructor() {
    const dependencies: DaemonSupervisorDependencies<
      FakeProcess,
      FakeRpc,
      FakeTimer,
      FakeStatus
    > = {
      now: () => this.now,
      schedule: (callback, delay) => {
        const timer = { callback, delay, cancelled: false };
        this.timers.push(timer);
        return timer;
      },
      cancel: (timer) => (timer.cancelled = true),
      runtimeCount: () => this.runtimeCount,
      resolveBinary: async () => {
        this.lifecycle.push("resolve");
        return this.resolveBinaryHook?.() ?? "fake-daemon";
      },
      spawn: () => {
        const process = {
          id: ++this.spawnCount,
          exited: false,
          killed: false,
          exitListeners: [],
          exitHistory: [],
          exitWaiters: [],
        } satisfies FakeProcess;
        this.processes.push(process);
        this.lifecycle.push(`spawn:${process.id}`);
        return process;
      },
      createRpc: (process) => ({
        id: process.id,
        closed: false,
        closeListeners: [],
      }),
      waitForSpawn: async (process) => {
        if (this.failSpawnIds.has(process.id))
          throw new Error(`spawn failed ${process.id}`);
      },
      waitForExit: (process) =>
        process.exited
          ? Promise.resolve()
          : new Promise<void>((resolve) => process.exitWaiters.push(resolve)),
      onExit: (process, listener) => {
        process.exitListeners.push(listener);
        process.exitHistory.push(listener);
      },
      onRpcClose: (rpc, listener) => rpc.closeListeners.push(listener),
      rpcClosed: (rpc) => rpc.closed,
      closeRpc: (rpc) => {
        this.lifecycle.push(`rpc-close:${rpc.id}`);
        this.closeRpc(rpc);
      },
      kill: (process) => {
        process.killed = true;
        this.lifecycle.push(`kill:${process.id}`);
        if (this.autoExitOnKill) this.exit(process, null, "SIGTERM");
      },
      bindConnection: (_process, rpc) => this.lifecycle.push(`bind:${rpc.id}`),
      initializeProtocol: async (rpc) => {
        this.lifecycle.push(`protocol:${rpc.id}`);
      },
      initializeWorkspaces: async (rpc) => {
        this.lifecycle.push(`workspaces:${rpc.id}`);
        return `daemon-${rpc.id}`;
      },
      replayDocuments: async (rpc) => {
        this.lifecycle.push(`documents:${rpc.id}`);
        this.replayDocumentsHook?.();
      },
      replayPreviews: async (rpc) => {
        this.lifecycle.push(`previews:${rpc.id}`);
        const process = this.processes.find(({ id }) => id === rpc.id);
        assert.ok(process);
        await this.replayPreviewsHook?.(process);
        return { documents: 2, previews: 1 };
      },
      resetAdapterState: () => {
        this.resetCount += 1;
        this.lifecycle.push("reset");
      },
      requestShutdown: async (rpc) => {
        this.shutdownCount += 1;
        this.lifecycle.push(`shutdown-request:${rpc.id}`);
        if (this.rejectShutdown) throw new Error("shutdown rejected");
      },
      notifyExit: (rpc) => this.lifecycle.push(`exit-notify:${rpc.id}`),
      log: (message) => this.logs.push(message),
      report: (error) => this.reports.push(error),
      showRecoveryStatus: () => this.status("recovering"),
      showRecoveredStatus: () => this.status("recovered"),
      showRepeatedFailure: async () => {
        this.repeatedFailureCount += 1;
        return this.repeatedFailureHook
          ? this.repeatedFailureHook()
          : this.repeatedFailureChoice;
      },
      openOutput: () => (this.openOutputCount += 1),
      disposeStatus: (status) => {
        status.disposed = true;
        this.lifecycle.push(`status-dispose:${status.kind}`);
      },
    };
    this.supervisor = new DaemonSupervisor(dependencies);
  }

  closeRpc(rpc: FakeRpc): void {
    if (rpc.closed) return;
    rpc.closed = true;
    for (const listener of rpc.closeListeners) listener();
  }

  exit(process: FakeProcess, code: number | null, signal: string | null): void {
    if (process.exited) return;
    process.exited = true;
    for (const listener of process.exitListeners.splice(0))
      listener(code, signal);
    for (const resolve of process.exitWaiters.splice(0)) resolve();
  }

  invokeLateExit(process: FakeProcess): void {
    for (const listener of process.exitHistory) listener(99, null);
  }

  status(kind: FakeStatus["kind"]): FakeStatus {
    const status = { kind, disposed: false };
    this.statuses.push(status);
    return status;
  }

  fireNextTimer(): number {
    const timer = this.timers.find((candidate) => !candidate.cancelled);
    assert.ok(timer, "expected a scheduled recovery timer");
    timer.cancelled = true;
    timer.callback();
    return timer.delay;
  }

  async settle(): Promise<void> {
    await new Promise<void>((resolve) => setImmediate(resolve));
    await new Promise<void>((resolve) => setImmediate(resolve));
  }
}

function runningProcess(harness: SupervisorHarness): FakeProcess {
  const process = harness.supervisor.process;
  assert.ok(process);
  return process;
}

async function reachRepeatedFailure(harness: SupervisorHarness): Promise<void> {
  await harness.supervisor.ensure();
  for (let attempt = 1; attempt <= 5; attempt += 1) {
    harness.exit(runningProcess(harness), attempt, null);
    harness.fireNextTimer();
    await harness.settle();
  }
  harness.exit(runningProcess(harness), 6, null);
  await harness.settle();
}

export const suiteName = "Daemon supervisor";

export function suite(): void {
  test("coalesces a failed spawn, clears state, and permits a fresh attempt", async () => {
    const harness = new SupervisorHarness();
    harness.failSpawnIds.add(1);

    const concurrent = await Promise.allSettled([
      harness.supervisor.ensure(),
      harness.supervisor.ensure(),
    ]);
    assert.deepEqual(
      concurrent.map(({ status }) => status),
      ["rejected", "rejected"],
    );
    assert.equal(harness.spawnCount, 1);
    assert.equal(harness.supervisor.process, undefined);
    assert.equal(harness.supervisor.rpc, undefined);

    harness.failSpawnIds.clear();
    await harness.supervisor.ensure();
    assert.equal(harness.spawnCount, 2);
    assert.equal(runningRpc(harness).id, 2);
  });

  test("uses the exact five recovery delays and stops before a sixth retry", async () => {
    const harness = new SupervisorHarness();
    harness.repeatedFailureChoice = "openOutput";
    await harness.supervisor.ensure();
    const delays: number[] = [];

    for (let attempt = 1; attempt <= 5; attempt += 1) {
      harness.exit(runningProcess(harness), attempt, null);
      assert.equal(retryCount(harness), attempt);
      delays.push(harness.fireNextTimer());
      await harness.settle();
    }
    harness.exit(runningProcess(harness), 6, null);
    await harness.settle();

    assert.deepEqual(delays, [250, 500, 1_000, 2_000, 4_000]);
    assert.equal(retryCount(harness), 6);
    assert.equal(harness.repeatedFailureCount, 1);
    assert.equal(harness.openOutputCount, 1);
    assert.equal(harness.timers.filter((timer) => !timer.cancelled).length, 0);
    assert.deepEqual(
      harness.logs.filter((message) => message.includes("retry scheduled")),
      delays.map(
        (delay, index) =>
          `[recovery-${index + 1}] retry scheduled in ${delay}ms`,
      ),
    );
    assert.equal(
      harness.statuses.filter(({ kind }) => kind === "recovering").length,
      6,
    );
    assert.equal(
      harness.statuses.filter(({ kind }) => kind === "recovered").length,
      5,
    );
    assert.ok(harness.statuses.every(({ disposed }) => disposed));
  });

  test("does not reset at thirty seconds and resets only after it", async () => {
    const harness = new SupervisorHarness();
    await harness.supervisor.ensure();
    const firstProcess = runningProcess(harness);
    harness.exit(firstProcess, 1, null);
    assert.equal(harness.fireNextTimer(), 250);
    await harness.settle();

    harness.now = 30_000;
    const boundaryProcess = runningProcess(harness);
    harness.exit(boundaryProcess, 2, null);
    assert.equal(retryCount(harness), 2);
    assert.equal(harness.fireNextTimer(), 500);
    await harness.settle();

    harness.now = 60_001;
    const afterBoundaryProcess = runningProcess(harness);
    harness.exit(afterBoundaryProcess, 3, null);
    assert.equal(retryCount(harness), 1);
    assert.equal(harness.fireNextTimer(), 250);
    assert.equal(
      harness.logs.at(-2),
      "[recovery-3] daemon exited code=3 signal=none; attempt=1",
    );
  });

  test("keeps a live daemon when runtime membership changes and ignores an old exit", async () => {
    const harness = new SupervisorHarness();
    await harness.supervisor.ensure();
    harness.runtimeCount = 2;
    await harness.supervisor.ensure();
    assert.equal(harness.spawnCount, 1);
    const first = runningProcess(harness);
    harness.exit(first, 1, null);
    harness.fireNextTimer();
    await harness.settle();
    harness.invokeLateExit(first);
    assert.equal(runningRpc(harness).id, 2);
    assert.equal(
      harness.timers.filter(({ cancelled }) => !cancelled).length,
      0,
    );
  });

  test("does not spawn from a pending start or active timer after disposal", async () => {
    const pending = new SupervisorHarness();
    let resolveBinary: ((binary: string) => void) | undefined;
    pending.resolveBinaryHook = () =>
      new Promise<string>((resolve) => (resolveBinary = resolve));
    const starting = pending.supervisor.ensure();
    await pending.settle();
    assert.deepEqual(pending.lifecycle, ["resolve"]);
    pending.supervisor.beginDispose();
    pending.supervisor.dispose();
    resolveBinary?.("fake-daemon");
    await starting;
    await pending.supervisor.ensure();
    assert.equal(pending.spawnCount, 0);

    const scheduled = new SupervisorHarness();
    await scheduled.supervisor.ensure();
    const process = runningProcess(scheduled);
    scheduled.exit(process, 1, null);
    const recoveryStatus = scheduled.statuses.at(-1);
    assert.ok(recoveryStatus);
    scheduled.supervisor.beginDispose();
    scheduled.supervisor.dispose();
    await scheduled.supervisor.ensure();
    assert.equal(scheduled.spawnCount, 1);
    assert.ok(scheduled.timers.every(({ cancelled }) => cancelled));
    assert.equal(recoveryStatus.disposed, true);
  });

  test("does not mark an exited preview replay ready until its replacement is ready", async () => {
    const harness = new SupervisorHarness();
    let interrupted = false;
    let finishReplacementReplay: (() => void) | undefined;
    harness.replayPreviewsHook = (process) => {
      if (!interrupted) {
        interrupted = true;
        harness.exit(process, 9, null);
        return;
      }
      return new Promise<void>(
        (resolve) => (finishReplacementReplay = resolve),
      );
    };
    const starting = harness.supervisor.ensure();
    await harness.settle();

    assert.equal(harness.spawnCount, 2);
    assert.equal(runningRpc(harness).id, 2);
    assert.equal(
      harness.logs.some((message) => message.includes("daemon ready")),
      false,
    );
    assert.deepEqual(
      harness.statuses.map(({ kind }) => kind),
      ["recovering"],
    );

    finishReplacementReplay?.();
    await starting;
    assert.equal(runningRpc(harness).id, 2);
    assert.equal(
      harness.logs.filter((message) => message.includes("daemon ready")).length,
      1,
    );
    assert.deepEqual(
      harness.statuses.map(({ kind }) => kind),
      ["recovering", "recovered"],
    );
  });

  test("waits for explicit exit before restart and never relaunches after stop", async () => {
    const harness = new SupervisorHarness();
    harness.autoExitOnKill = false;
    await harness.supervisor.ensure();
    harness.closeRpc(runningRpc(harness));
    const restarting = harness.supervisor.ensure();
    await harness.settle();
    const first = harness.processes[0];
    assert.equal(first.killed, true);
    assert.equal(harness.spawnCount, 1);
    harness.exit(first, null, "SIGTERM");
    await restarting;
    assert.equal(harness.spawnCount, 2);
    assert.ok(
      harness.lifecycle.indexOf("kill:1") < harness.lifecycle.indexOf("reset"),
    );
    assert.ok(
      harness.lifecycle.indexOf("reset") < harness.lifecycle.indexOf("spawn:2"),
    );
    assert.equal(
      harness.lifecycle.filter((event) => event === "reset").length,
      1,
    );

    for (const dispose of [false, true]) {
      const stopped = new SupervisorHarness();
      stopped.autoExitOnKill = false;
      await stopped.supervisor.ensure();
      stopped.closeRpc(runningRpc(stopped));
      const operation = stopped.supervisor.ensure();
      await stopped.settle();
      const process = stopped.processes[0];
      stopped.runtimeCount = 0;
      if (dispose) {
        stopped.supervisor.beginDispose();
        stopped.supervisor.dispose();
      } else stopped.supervisor.stop();
      assert.equal(stopped.supervisor.process, undefined);
      assert.equal(stopped.supervisor.rpc, undefined);
      stopped.exit(process, null, "SIGTERM");
      await operation;
      await stopped.settle();
      assert.equal(stopped.spawnCount, 1);
    }
  });

  test("handles every repeated-failure dialog outcome and ignores stale outcomes", async () => {
    const dismissed = new SupervisorHarness();
    await reachRepeatedFailure(dismissed);
    assert.equal(dismissed.spawnCount, 6);
    assert.equal(dismissed.openOutputCount, 0);
    assert.equal(
      dismissed.timers.filter(({ cancelled }) => !cancelled).length,
      0,
    );
    assert.ok(dismissed.statuses.every(({ disposed }) => disposed));

    const retried = new SupervisorHarness();
    retried.repeatedFailureChoice = "retry";
    await reachRepeatedFailure(retried);
    await retried.settle();
    assert.equal(retried.spawnCount, 7);
    assert.equal(
      retried.logs.filter((message) => message.includes("daemon ready")).length,
      7,
    );
    assert.equal(
      retried.timers.filter(({ cancelled }) => !cancelled).length,
      0,
    );

    const rejected = new SupervisorHarness();
    rejected.repeatedFailureHook = () => Promise.reject(new Error("dialog"));
    await reachRepeatedFailure(rejected);
    assert.deepEqual(
      rejected.reports.map((error) => errorMessage(error)),
      ["dialog"],
    );

    const disposed = new SupervisorHarness();
    let rejectDialog: ((error: Error) => void) | undefined;
    disposed.repeatedFailureHook = () =>
      new Promise((_resolve, reject) => (rejectDialog = reject));
    await reachRepeatedFailure(disposed);
    disposed.supervisor.beginDispose();
    disposed.supervisor.dispose();
    rejectDialog?.(new Error("late dialog"));
    await disposed.settle();
    assert.deepEqual(disposed.reports, []);
    assert.equal(disposed.openOutputCount, 0);

    const stale = new SupervisorHarness();
    let resolveDialog:
      ((choice: "retry" | "openOutput" | undefined) => void) | undefined;
    stale.repeatedFailureHook = () =>
      new Promise((resolve) => (resolveDialog = resolve));
    await reachRepeatedFailure(stale);
    await stale.supervisor.ensure();
    assert.equal(stale.spawnCount, 7);
    resolveDialog?.("openOutput");
    await stale.settle();
    assert.equal(stale.openOutputCount, 0);
  });

  test("disposes in order when the shutdown request fails", async () => {
    const harness = new SupervisorHarness();
    harness.rejectShutdown = true;
    await harness.supervisor.ensure();
    const process = runningProcess(harness);
    harness.supervisor.beginDispose();
    harness.supervisor.dispose();
    harness.supervisor.dispose();
    harness.invokeLateExit(process);
    await harness.settle();

    assert.equal(harness.shutdownCount, 1);
    assert.equal(harness.supervisor.process, undefined);
    assert.equal(harness.supervisor.rpc, undefined);
    assert.equal(harness.timers.length, 0);
    assert.deepEqual(harness.lifecycle.slice(-4), [
      "shutdown-request:1",
      "exit-notify:1",
      "rpc-close:1",
      "kill:1",
    ]);
  });
}

function retryCount(harness: SupervisorHarness): number {
  return Number(
    /attempt=(\d+)/.exec(
      harness.logs.findLast((message) => message.includes("attempt=")) ?? "",
    )?.[1] ?? 0,
  );
}

function runningRpc(harness: SupervisorHarness): FakeRpc {
  const rpc = harness.supervisor.rpc;
  assert.ok(rpc);
  return rpc;
}
