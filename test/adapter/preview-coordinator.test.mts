import * as assert from "node:assert/strict";

import type { DaemonOrigin } from "../../adapters/vscode/src/document-coordinator.mjs";
import {
  type PreviewCandidate,
  type PreviewEchoState,
  type PreviewLifecycleDependencies,
  UnmatchedPreviewEventQueue,
  applySourceNavigationLifecycle,
  beginEmbeddedPreviewHandshake,
  completePreviewReadiness,
  conservativePreviewEventBytes,
  disposePreviewCandidate,
  disposePreviewLifecycle,
  embeddedPreviewShell,
  handleEditorSelectionLifecycle,
  handleEditorViewportLifecycle,
  handlePreviewEventLifecycle,
  openPreviewLifecycle,
  previewCandidateIsCurrent,
  previewEventUtf8Bytes,
  recreatePreviewsLifecycle,
  reloadPreviewLifecycle,
  requestPreviewCandidate,
} from "../../adapters/vscode/src/preview-coordinator.mjs";
import type { JsonRpcConnection } from "../../adapters/vscode/src/rpc.mjs";
import type {
  DocumentState,
  PreviewState,
  WorkspaceRuntime,
} from "../../adapters/vscode/src/runtime-state.mjs";

export const suiteName = "Preview coordinator";

export function suite(): void {
  test("rejects every stale create-preview candidate dimension", () => {
    const rpc = {} as JsonRpcConnection;
    const origin: DaemonOrigin = {
      rpc,
      daemonInstanceId: "daemon-1",
      generation: 7,
    };
    const documentState: DocumentState = {
      sessionId: "document-1",
      version: 3,
    };
    const runtime = {
      removed: false,
      documents: new Map([["file:///note.md", documentState]]),
      previews: new Map(),
    } as unknown as WorkspaceRuntime;
    const candidate = {
      result: { previewSessionId: "preview-1" },
      origin,
      runtime,
      documentUri: "file:///note.md",
      documentState,
      documentSessionId: "document-1",
      documentVersion: 3,
    } as PreviewCandidate;
    const current = {
      disposed: false,
      runtime,
      origin,
      documentState,
      documentVersion: 3,
    };
    assert.equal(previewCandidateIsCurrent(candidate, current), true);

    const staleStates = [
      { ...current, disposed: true },
      { ...current, runtime: {} as WorkspaceRuntime },
      { ...current, origin: { ...origin, rpc: {} as JsonRpcConnection } },
      { ...current, origin: { ...origin, generation: 8 } },
      { ...current, origin: { ...origin, daemonInstanceId: "daemon-2" } },
      { ...current, documentState: { ...documentState } },
      { ...current, documentState: { ...documentState, sessionId: "other" } },
      { ...current, documentVersion: 4 },
    ];
    for (const stale of staleStates)
      assert.equal(previewCandidateIsCurrent(candidate, stale), false);
    runtime.removed = true;
    assert.equal(previewCandidateIsCurrent(candidate, current), false);
    runtime.removed = false;
    Object.assign(rpc, { closed: true });
    assert.equal(previewCandidateIsCurrent(candidate, current), false);
  });

  test("disposes a rejected candidate through its origin identity", async () => {
    const calls: unknown[] = [];
    const rpc = {
      request(method: string, params: unknown) {
        calls.push({ method, params });
        return Promise.resolve(null);
      },
    } as unknown as JsonRpcConnection;
    const candidate = {
      result: { previewSessionId: "preview-old" },
      origin: { rpc, daemonInstanceId: "daemon-old", generation: 2 },
    } as PreviewCandidate;
    await disposePreviewCandidate(candidate);
    assert.deepEqual(calls, [
      {
        method: "fleximark/disposePreview",
        params: {
          daemonInstanceId: "daemon-old",
          previewSessionId: "preview-old",
        },
      },
    ]);
  });

  test("builds the exact CSP shell without embedding a publication", () => {
    const shell = embeddedPreviewShell(
      "nonce",
      "token",
      "vscode-webview:",
      "vscode-webview:/host.js",
      "body{}",
    );
    assert.equal(
      shell,
      '<!doctype html><html><head><meta charset="utf-8"><meta http-equiv="Content-Security-Policy" content="default-src \'none\'; script-src \'nonce-nonce\' vscode-webview:; style-src vscode-webview: \'unsafe-inline\'; img-src vscode-webview: data: blob:; media-src vscode-webview: blob:; frame-src https://www.youtube-nocookie.com; object-src \'none\';"><meta name="viewport" content="width=device-width,initial-scale=1"><meta name="fleximark-message-token" content="token"><style>body{}</style></head><body><main id="preview" class="markdown-body"></main><script nonce="nonce" src="vscode-webview:/host.js"></script></body></html>',
    );
    assert.doesNotMatch(shell, /initialPublication|documentVersion|html:/);
  });

  test("rejects a stale create response before creating any UI", async () => {
    const { candidate, runtime } = previewCandidate();
    let rejected = 0;
    let panels = 0;
    await openPreviewLifecycle(
      "embeddedHtml",
      previewDependencies(candidate, runtime, {
        candidateCurrent: () => false,
        rejectCandidate: async () => {
          rejected += 1;
        },
        createPanel: () => {
          panels += 1;
          throw new Error("must not create UI");
        },
      }),
    );
    assert.equal(rejected, 1);
    assert.equal(panels, 0);
    assert.equal(runtime.previews.size, 0);
  });

  test("cleans the origin candidate when embedded UI construction throws", async () => {
    const { candidate, runtime } = previewCandidate();
    const rejected: string[] = [];
    await assert.rejects(
      openPreviewLifecycle(
        "embeddedHtml",
        previewDependencies(candidate, runtime, {
          createPanel: () => {
            throw new Error("panel failed");
          },
          rejectCandidate: async (item) => {
            rejected.push(item.result.previewSessionId);
          },
        }),
      ),
      /panel failed/,
    );
    assert.deepEqual(rejected, ["preview"]);
    assert.equal(runtime.previews.size, 0);
  });

  test("keeps the committed external preview when opening the browser fails", async () => {
    const { candidate, runtime } = previewCandidate();
    const queue = new UnmatchedPreviewEventQueue();
    let navigations = 0;
    await assert.rejects(
      openPreviewLifecycle(
        "externalBrowser",
        previewDependencies(candidate, runtime, {
          openExternal: async () => {
            await handlePreviewEventLifecycle(
              candidate.origin,
              sourceNavigationEvent("preview"),
              [runtime],
              queue,
              {
                reload: assert.fail,
                navigate: async () => {
                  navigations += 1;
                },
              },
            );
            throw new Error("browser failed");
          },
          discardQueued: (origin, previewSessionId) => {
            queue.take(origin, previewSessionId);
          },
        }),
      ),
      /browser failed/,
    );
    assert.equal(runtime.previews.get("preview")?.target, "externalBrowser");
    assert.equal(runtime.previews.get("preview")?.ready, true);
    assert.equal(queue.take(candidate.origin, "preview"), undefined);
    assert.equal(navigations, 0);
  });

  test("external failure discard cannot consume queue-wide overflow recovery", async () => {
    const { candidate, runtime } = previewCandidate();
    const queue = globallyLimitedQueue({ maxStreams: 1 });
    queue.enqueue(candidate.origin, previewEvent("other", "full", 1));
    assert.deepEqual(
      queue.enqueue(candidate.origin, previewEvent("overflow", "full", 1)),
      { status: "overflow", origins: [candidate.origin] },
    );

    await assert.rejects(
      openPreviewLifecycle(
        "externalBrowser",
        previewDependencies(candidate, runtime, {
          openExternal: async () => {
            throw new Error("browser failed");
          },
          discardQueued: (origin, previewSessionId) => {
            queue.take(origin, previewSessionId);
          },
        }),
      ),
      /browser failed/,
    );

    assert.equal(queue.usage.failClosed, true);
    assert.deepEqual(queue.takeForDelivery(candidate.origin, "other-preview"), {
      events: [],
      reloadRequired: true,
    });
    queue.clearOrigin(candidate.origin);
    assert.equal(queue.usage.failClosed, false);
  });

  test("opens the external URL after commit but does not replay after removal while pending", async () => {
    const { candidate, runtime } = previewCandidate();
    let releaseOpen!: () => void;
    let observedOpen!: () => void;
    const pendingOpen = new Promise<void>((resolve) => {
      releaseOpen = resolve;
    });
    const opened = new Promise<void>((resolve) => {
      observedOpen = resolve;
    });
    let activations = 0;
    let navigations = 0;
    const queue = new UnmatchedPreviewEventQueue();
    const opening = openPreviewLifecycle(
      "externalBrowser",
      previewDependencies(candidate, runtime, {
        openExternal: async () => {
          assert.equal(runtime.previews.get("preview")?.ready, false);
          observedOpen();
          await pendingOpen;
        },
        previewCurrent: (owner, preview) =>
          !owner.removed && owner.previews.get("preview") === preview,
        activate: async () => {
          activations += 1;
        },
        discardQueued: (origin, previewSessionId) => {
          queue.take(origin, previewSessionId);
        },
      }),
    );
    await opened;
    await handlePreviewEventLifecycle(
      candidate.origin,
      sourceNavigationEvent("preview"),
      [runtime],
      queue,
      {
        reload: assert.fail,
        navigate: async () => {
          navigations += 1;
        },
      },
    );
    assert.equal(navigations, 0);
    runtime.removed = true;
    runtime.previews.clear();
    releaseOpen();
    await opening;
    assert.equal(activations, 0);
    assert.equal(navigations, 0);
  });

  test("queues external navigation until open succeeds and then drains only while current", async () => {
    const { candidate, runtime } = previewCandidate();
    let releaseOpen!: () => void;
    let observedOpen!: () => void;
    const pendingOpen = new Promise<void>((resolve) => {
      releaseOpen = resolve;
    });
    const opened = new Promise<void>((resolve) => {
      observedOpen = resolve;
    });
    const queue = new UnmatchedPreviewEventQueue();
    let navigations = 0;
    const readiness = () =>
      actualReadinessDependencies(runtime, queue, {
        navigate: async () => {
          navigations += 1;
        },
      });
    const opening = openPreviewLifecycle(
      "externalBrowser",
      previewDependencies(candidate, runtime, {
        openExternal: async () => {
          observedOpen();
          await pendingOpen;
        },
        previewCurrent: (owner, preview) =>
          !owner.removed && owner.previews.get("preview") === preview,
        activate: (owner, preview) =>
          completePreviewReadiness(owner, preview, readiness()),
        discardQueued: (origin, previewSessionId) => {
          queue.take(origin, previewSessionId);
        },
      }),
    );
    await opened;
    await handlePreviewEventLifecycle(
      candidate.origin,
      sourceNavigationEvent("preview"),
      [runtime],
      queue,
      { reload: assert.fail, navigate: async () => assert.fail() },
    );
    assert.equal(navigations, 0);
    releaseOpen();
    await opening;
    assert.equal(navigations, 1);
    assert.equal(runtime.previews.get("preview")?.ready, true);
  });

  test("holds a drifted initial external preview for a full event after reload success or rejection", async () => {
    for (const outcome of ["success", "reject"] as const) {
      const { candidate: fixture, runtime } = previewCandidate();
      const reloadFailure = new Error("initial external reload rejected");
      const reloadCalls: unknown[] = [];
      const rpc = {
        closed: false,
        request(method: string, params: unknown) {
          reloadCalls.push({ method, params });
          return Promise.resolve(null);
        },
      } as unknown as JsonRpcConnection;
      const candidate = {
        ...fixture,
        origin: daemonOrigin(rpc, "daemon", 1),
      } as PreviewCandidate;
      const queue = new UnmatchedPreviewEventQueue();
      let versionCurrent = true;
      let reloadAttempts = 0;
      let activations = 0;
      let disposals = 0;
      const reports: string[] = [];
      const activate = async (owner: WorkspaceRuntime, item: PreviewState) => {
        activations += 1;
        await completePreviewReadiness(
          owner,
          item,
          actualReadinessDependencies(owner, queue),
        );
      };

      await openPreviewLifecycle(
        "externalBrowser",
        previewDependencies(candidate, runtime, {
          candidateCurrent: () => versionCurrent,
          openExternal: async () => {
            versionCurrent = false;
          },
          previewCurrent: (owner, item) =>
            !owner.removed &&
            owner.previews.get(item.previewSessionId) === item &&
            item.originRpc === candidate.origin.rpc &&
            item.originGeneration === candidate.origin.generation &&
            item.originDaemonInstanceId === candidate.origin.daemonInstanceId,
          markReload: (origin, previewSessionId) =>
            queue.markReloadRequired(origin, previewSessionId),
          reload: async (owner, item) => {
            reloadAttempts += 1;
            if (outcome === "reject") throw reloadFailure;
            return reloadPreviewLifecycle(owner, item, {
              disposed: () => false,
              currentOrigin: () => candidate.origin,
              reportFailure: (message) => reports.push(message),
            });
          },
          report: (error) => reports.push(String(error)),
          activate,
          dispose: async (owner, item) => {
            disposals += 1;
            owner.previews.delete(item.previewSessionId);
          },
        }),
      );

      const preview = runtime.previews.get("preview");
      assert.ok(preview);
      assert.equal(reloadAttempts, 1, outcome);
      assert.deepEqual(
        reloadCalls,
        outcome === "success"
          ? [
              {
                method: "fleximark/reloadPreview",
                params: {
                  daemonInstanceId: "daemon",
                  previewSessionId: "preview",
                },
              },
            ]
          : [],
        outcome,
      );
      assert.equal(activations, 0, outcome);
      assert.equal(disposals, 0, outcome);
      assert.equal(preview.ready, false, outcome);
      assert.equal(preview.reloadPending, true, outcome);
      assert.equal(queue.usage.reloadMarkers, 1, outcome);
      assert.deepEqual(
        reports,
        outcome === "reject" ? ["Error: initial external reload rejected"] : [],
        outcome,
      );

      let reactivation: Promise<void> | undefined;
      await handlePreviewEventLifecycle(
        candidate.origin,
        previewEvent("preview", "full", 2),
        [runtime],
        queue,
        {
          reload: () => assert.fail("full recovery must not reload again"),
          navigate: async () => assert.fail("full event is not navigation"),
          reactivate: (owner, item) => {
            reactivation = activate(owner, item);
            return reactivation;
          },
        },
      );
      assert.ok(reactivation);
      await reactivation;
      assert.equal(activations, 1, outcome);
      assert.equal(preview.ready, true, outcome);
      assert.equal(preview.reloadPending, false, outcome);
      assert.equal(queue.usage.streams, 0, outcome);
    }
  });

  test("keeps an initial external preview for replay when its origin closes during URL open", async () => {
    const { candidate, runtime } = previewCandidate();
    let releaseOpen!: () => void;
    let observeOpen!: () => void;
    const pendingOpen = new Promise<void>((resolve) => {
      releaseOpen = resolve;
    });
    const opened = new Promise<void>((resolve) => {
      observeOpen = resolve;
    });
    let activations = 0;
    let disposals = 0;
    let reloads = 0;

    const opening = openPreviewLifecycle(
      "externalBrowser",
      previewDependencies(candidate, runtime, {
        candidateIdentityCurrent: () => !candidate.origin.rpc.closed,
        openExternal: async () => {
          observeOpen();
          await pendingOpen;
        },
        activate: async () => {
          activations += 1;
        },
        reload: async () => {
          reloads += 1;
          return true;
        },
        dispose: async () => {
          disposals += 1;
        },
      }),
    );
    await opened;
    Object.assign(candidate.origin.rpc, { closed: true });
    releaseOpen();
    await opening;

    const preview = runtime.previews.get("preview");
    assert.ok(preview);
    assert.equal(preview.originRpc, candidate.origin.rpc);
    assert.equal(activations, 0);
    assert.equal(reloads, 0);
    assert.equal(disposals, 0);
  });

  test("does not let an old recovery completion dispose a mutated preview incarnation", async () => {
    const { candidate, runtime } = previewCandidate();
    let versionCurrent = true;
    let observeReload!: () => void;
    let resolveReload!: (accepted: boolean) => void;
    const reloadStarted = new Promise<void>((resolve) => {
      observeReload = resolve;
    });
    const pendingReload = new Promise<boolean>((resolve) => {
      resolveReload = resolve;
    });
    const queue = new UnmatchedPreviewEventQueue();
    let disposals = 0;

    const opening = openPreviewLifecycle(
      "externalBrowser",
      previewDependencies(candidate, runtime, {
        candidateCurrent: () => versionCurrent,
        openExternal: async () => {
          versionCurrent = false;
        },
        markReload: (origin, previewSessionId) =>
          queue.markReloadRequired(origin, previewSessionId),
        reload: async () => {
          observeReload();
          return pendingReload;
        },
        activate: async () => assert.fail("drift requires a full event"),
        dispose: async () => {
          disposals += 1;
        },
      }),
    );
    await reloadStarted;
    const preview = runtime.previews.get("preview");
    assert.ok(preview);
    const replacementRpc = { closed: false } as JsonRpcConnection;
    runtime.previews.delete("preview");
    preview.previewSessionId = "replacement-preview";
    preview.originRpc = replacementRpc;
    preview.originDaemonInstanceId = "replacement-daemon";
    preview.originGeneration = 2;
    preview.ready = true;
    preview.reloadPending = false;
    runtime.previews.set("replacement-preview", preview);

    resolveReload(false);
    await opening;

    assert.equal(disposals, 0);
    assert.equal(runtime.previews.get("replacement-preview"), preview);
    assert.equal(preview.originRpc, replacementRpc);
    assert.equal(preview.originDaemonInstanceId, "replacement-daemon");
    assert.equal(preview.originGeneration, 2);
    assert.equal(preview.ready, true);
    assert.equal(preview.reloadPending, false);
  });

  test("ignores an initial external rejection after same-id origin replacement", async () => {
    const { candidate, runtime } = previewCandidate();
    let observeOpen!: () => void;
    let rejectOpen!: (error: Error) => void;
    const opened = new Promise<void>((resolve) => {
      observeOpen = resolve;
    });
    const pendingOpen = new Promise<void>((_resolve, reject) => {
      rejectOpen = reject;
    });
    const discarded: string[] = [];
    const opening = openPreviewLifecycle(
      "externalBrowser",
      previewDependencies(candidate, runtime, {
        openExternal: async () => {
          observeOpen();
          await pendingOpen;
        },
        discardQueued: (_origin, previewSessionId) => {
          discarded.push(previewSessionId);
        },
      }),
    );
    await opened;
    const preview = runtime.previews.get("preview");
    assert.ok(preview);
    const replacementRpc = { closed: false } as JsonRpcConnection;
    preview.originRpc = replacementRpc;
    preview.originDaemonInstanceId = "replacement-daemon";
    preview.originGeneration = 2;
    preview.ready = false;

    rejectOpen(new Error("browser failed"));
    await opening;

    assert.deepEqual(discarded, ["preview"]);
    assert.equal(runtime.previews.get("preview"), preview);
    assert.equal(preview.originRpc, replacementRpc);
    assert.equal(preview.ready, false);
  });

  test("reports ready-listener rejection only for the exact preview incarnation", async () => {
    const { candidate, runtime } = previewCandidate();
    let messageListener!: (event: unknown) => void;
    let disposeListener!: () => void;
    const panel = {
      dispose: () => undefined,
      onDidDispose(listener: () => void) {
        disposeListener = listener;
        return { dispose: () => undefined };
      },
      webview: {
        cspSource: "vscode-webview:",
        html: "",
        onDidReceiveMessage(listener: (event: unknown) => void) {
          messageListener = listener;
          return { dispose: () => undefined };
        },
      },
    };
    let rejectHandshake!: (error: Error) => void;
    const pendingHandshake = () =>
      new Promise<void>((resolve, reject) => {
        void resolve;
        rejectHandshake = reject;
      });
    let handshake = pendingHandshake();
    let disposals = 0;
    const reports: unknown[] = [];
    await openPreviewLifecycle(
      "embeddedHtml",
      previewDependencies(candidate, runtime, {
        createPanel: () => panel as never,
        handshake: () => handshake,
        previewCurrent: () => true,
        report: (error) => reports.push(error),
        dispose: () => {
          disposals += 1;
        },
      }),
    );

    const currentFailure = new Error("current ready failed");
    messageListener({ type: "ready" });
    rejectHandshake(currentFailure);
    await Promise.resolve();
    await Promise.resolve();
    assert.deepEqual(reports, [currentFailure]);

    handshake = pendingHandshake();
    const retiredFailure = new Error("retired ready failed");
    messageListener({ type: "ready" });
    const recreated = runtime.previews.get("preview");
    assert.ok(recreated);
    recreated.originRpc = {} as JsonRpcConnection;
    recreated.originGeneration += 1;
    recreated.originDaemonInstanceId = "replacement-daemon";
    rejectHandshake(retiredFailure);
    await Promise.resolve();
    await Promise.resolve();
    assert.deepEqual(reports, [currentFailure]);

    disposeListener();
    assert.equal(disposals, 1);
  });

  test("starts a fresh production listener handshake after a webview reload", async () => {
    const { candidate, runtime } = previewCandidate();
    let messageListener!: (event: unknown) => void;
    let posts = 0;
    const panel = {
      dispose: () => undefined,
      onDidDispose: () => ({ dispose: () => undefined }),
      webview: {
        cspSource: "vscode-webview:",
        html: "",
        onDidReceiveMessage(listener: (event: unknown) => void) {
          messageListener = listener;
          return { dispose: () => undefined };
        },
        postMessage: async () => {
          posts += 1;
          return true;
        },
      },
    };
    const queue = new UnmatchedPreviewEventQueue();
    let latestHandshake = Promise.resolve();
    await openPreviewLifecycle(
      "embeddedHtml",
      previewDependencies(candidate, runtime, {
        createPanel: () => panel as never,
        previewCurrent: (owner, preview) =>
          owner.previews.get(preview.previewSessionId) === preview,
        handshake: (owner, preview) => {
          latestHandshake = beginEmbeddedPreviewHandshake(
            owner,
            preview,
            readinessDependencies(owner, queue, []),
          );
          return latestHandshake;
        },
      }),
    );
    const preview = runtime.previews.get("preview");
    assert.ok(preview);

    messageListener({ type: "ready" });
    await latestHandshake;
    const firstEpoch = preview.handshakeEpoch;
    assert.equal(preview.ready, true);

    messageListener({ type: "ready" });
    await latestHandshake;
    assert.equal(posts, 2);
    assert.equal(preview.handshakeEpoch, (firstEpoch ?? 0) + 1);
    assert.equal(preview.ready, true);
  });

  test("routes same-chunk events through the production coordinator FIFO", async () => {
    const rpc = {} as JsonRpcConnection;
    const origin = daemonOrigin(rpc, "daemon", 1);
    const queue = new UnmatchedPreviewEventQueue();
    const runtime = { previews: new Map() } as unknown as WorkspaceRuntime;
    const events = [
      previewEvent("preview", "full", 1),
      previewEvent("preview", "patch", 2),
      previewEvent("preview", "viewport", 2),
    ];
    for (const event of events)
      await handlePreviewEventLifecycle(origin, event, [runtime], queue, {
        reload: () => assert.fail("must queue rather than reload"),
        navigate: async () => assert.fail("must queue rather than navigate"),
      });
    assert.deepEqual(queue.take(origin, "preview"), {
      events,
      reloadRequired: false,
    });
  });

  test("reports queue-wide overflow once for every affected origin", async () => {
    let firstCloses = 0;
    let secondCloses = 0;
    const firstRpcState = {
      closed: false,
      close() {
        firstCloses += 1;
        this.closed = true;
      },
    };
    const secondRpcState = {
      closed: false,
      close() {
        secondCloses += 1;
        this.closed = true;
      },
    };
    const firstRpc = firstRpcState as unknown as JsonRpcConnection;
    const secondRpc = secondRpcState as unknown as JsonRpcConnection;
    const firstOrigin = daemonOrigin(firstRpc, "daemon-first", 1);
    const secondOrigin = daemonOrigin(secondRpc, "daemon-second", 2);
    const queue = globallyLimitedQueue({ maxStreams: 2 });
    const runtime = { previews: new Map() } as unknown as WorkspaceRuntime;
    const overflowed: DaemonOrigin[] = [];
    const failClosedAfterCleanup: boolean[] = [];
    const dependencies = {
      reload: assert.fail,
      navigate: async () => assert.fail(),
      overflow: (overflowOrigin: DaemonOrigin) => {
        overflowed.push(overflowOrigin);
        overflowOrigin.rpc.close();
        queue.clearOrigin(overflowOrigin);
        failClosedAfterCleanup.push(queue.usage.failClosed);
      },
    };
    await handlePreviewEventLifecycle(
      firstOrigin,
      previewEvent("first", "full", 1, "daemon-first"),
      [runtime],
      queue,
      dependencies,
    );
    await handlePreviewEventLifecycle(
      secondOrigin,
      previewEvent("second", "full", 1, "daemon-second"),
      [runtime],
      queue,
      dependencies,
    );
    await handlePreviewEventLifecycle(
      firstOrigin,
      previewEvent("overflow", "full", 1, "daemon-first"),
      [runtime],
      queue,
      dependencies,
    );
    assert.deepEqual(overflowed, [firstOrigin, secondOrigin]);
    assert.equal(firstCloses, 1);
    assert.equal(secondCloses, 1);
    assert.equal(firstRpcState.closed, true);
    assert.equal(secondRpcState.closed, true);
    assert.deepEqual(failClosedAfterCleanup, [true, false]);
    assert.equal(queue.usage.failClosed, false);
    const replacement = previewEvent("replacement", "full", 1, "daemon-new");
    const replacementOrigin = daemonOrigin(
      {} as JsonRpcConnection,
      "daemon-new",
      3,
    );
    assert.equal(queue.enqueue(replacementOrigin, replacement), "stored");
    assert.deepEqual(queue.take(replacementOrigin, "replacement")?.events, [
      replacement,
    ]);
  });

  test("runs exactly one subsequent initialize for ready received during a successful post", async () => {
    let resolvePost!: (delivered: boolean) => void;
    const post = new Promise<boolean>((resolve) => {
      resolvePost = resolve;
    });
    let posts = 0;
    const rpc = { closed: false } as JsonRpcConnection;
    const origin = daemonOrigin(rpc, "daemon", 2);
    const queue = new UnmatchedPreviewEventQueue();
    const preview = previewState(origin, {
      postMessage: () => {
        posts += 1;
        return post;
      },
    });
    const runtime = {
      removed: false,
      previews: new Map([["preview", preview]]),
    } as unknown as WorkspaceRuntime;
    const delivered: string[] = [];
    const dependencies = readinessDependencies(runtime, queue, delivered);
    const first = beginEmbeddedPreviewHandshake(runtime, preview, dependencies);
    const repeated = beginEmbeddedPreviewHandshake(
      runtime,
      preview,
      dependencies,
    );
    assert.equal(first, repeated);
    queue.enqueue(origin, previewEvent("preview", "full", 1));
    queue.enqueue(origin, previewEvent("preview", "patch", 2));
    queue.enqueue(origin, previewEvent("preview", "viewport", 2));
    assert.equal(preview.ready, false);
    resolvePost(true);
    await first;
    assert.equal(posts, 2);
    assert.deepEqual(delivered, ["full", "patch", "viewport"]);
    assert.equal(preview.ready, true);
    const completedEpoch = preview.handshakeEpoch;
    await beginEmbeddedPreviewHandshake(runtime, preview, dependencies);
    assert.equal(posts, 3);
    assert.equal(preview.handshakeEpoch, (completedEpoch ?? 0) + 1);
  });

  test("retries one pending ready after a coalesced initial post failure", async () => {
    for (const failure of ["false", "reject"] as const) {
      let resolveFirst!: (delivered: boolean) => void;
      let rejectFirst!: (error: Error) => void;
      const firstPost = new Promise<boolean>((resolve, reject) => {
        resolveFirst = resolve;
        rejectFirst = reject;
      });
      let initializePosts = 0;
      const origin = daemonOrigin(
        { closed: false } as JsonRpcConnection,
        "daemon",
        1,
      );
      const preview = previewState(origin, {
        postMessage: () => {
          initializePosts += 1;
          return initializePosts === 1 ? firstPost : Promise.resolve(true);
        },
      });
      const runtime = {
        removed: false,
        previews: new Map([["preview", preview]]),
      } as unknown as WorkspaceRuntime;
      let reloads = 0;
      const reports: unknown[] = [];
      const queue = new UnmatchedPreviewEventQueue();
      const dependencies = {
        ...readinessDependencies(runtime, queue, []),
        reload: async () => {
          reloads += 1;
          return true;
        },
        report: (error: unknown) => reports.push(error),
      };
      const first = beginEmbeddedPreviewHandshake(
        runtime,
        preview,
        dependencies,
      );
      const repeated = beginEmbeddedPreviewHandshake(
        runtime,
        preview,
        dependencies,
      );
      assert.equal(first, repeated);
      const error = new Error("initial post failed");
      if (failure === "reject") rejectFirst(error);
      else resolveFirst(false);
      await first;

      assert.equal(initializePosts, 2);
      assert.equal(reloads, 2);
      assert.equal(preview.ready, false);
      assert.equal(preview.reloadPending, true);
      assert.equal(queue.usage.reloadMarkers, 1);
      assert.deepEqual(reports, failure === "reject" ? [error] : []);
    }
  });

  test("auto-reactivates queued publication failure on the next authoritative full event", async () => {
    for (const failure of ["false", "reject"] as const) {
      const origin = daemonOrigin(
        { closed: false } as JsonRpcConnection,
        "daemon",
        1,
      );
      const publications: string[] = [];
      let publicationAttempts = 0;
      const preview = previewState(origin, {
        postMessage: (message: { type: string; event?: { type: string } }) => {
          assert.equal(message.type, "previewEvent");
          publications.push(message.event?.type ?? "missing");
          publicationAttempts += 1;
          if (publicationAttempts > 1) return Promise.resolve(true);
          return failure === "false"
            ? Promise.resolve(false)
            : Promise.reject(new Error("publication failed"));
        },
      });
      const runtime = {
        removed: false,
        previews: new Map([["preview", preview]]),
      } as unknown as WorkspaceRuntime;
      const queue = new UnmatchedPreviewEventQueue();
      queue.enqueue(origin, previewEvent("preview", "full", 2));
      let reloads = 0;
      const readiness = actualReadinessDependencies(runtime, queue, {
        reload: () => {
          reloads += 1;
          return true;
        },
      });

      await completePreviewReadiness(runtime, preview, readiness);
      assert.equal(reloads, 1);
      assert.equal(preview.ready, false);
      assert.equal(preview.reloadPending, true);
      assert.equal(queue.usage.reloadMarkers, 1);

      let reactivation: Promise<void> | undefined;
      await handlePreviewEventLifecycle(
        origin,
        previewEvent("preview", "full", 3),
        [runtime],
        queue,
        {
          reload: () => assert.fail("full recovery must not reload again"),
          navigate: async () => assert.fail("full event is not navigation"),
          reactivate: (owner, item) => {
            reactivation = beginEmbeddedPreviewHandshake(
              owner,
              item,
              readiness,
            );
            return reactivation;
          },
        },
      );
      assert.ok(reactivation);
      await reactivation;
      assert.deepEqual(publications, ["full", "full"]);
      assert.equal(preview.ready, true);
      assert.equal(preview.reloadPending, false);
      assert.equal(queue.usage.streams, 0);
    }
  });

  test("retains a publication recovery marker when reload rejects and skips stale initialization", async () => {
    const origin = daemonOrigin(
      { closed: false } as JsonRpcConnection,
      "daemon",
      1,
    );
    const publicationTypes: string[] = [];
    const preview = previewState(origin, {
      postMessage: (message: { type: string; event?: { type: string } }) => {
        assert.equal(message.type, "previewEvent");
        publicationTypes.push(message.event?.type ?? "missing");
        return Promise.resolve(publicationTypes.length > 1);
      },
    });
    const runtime = {
      removed: false,
      previews: new Map([["preview", preview]]),
    } as unknown as WorkspaceRuntime;
    const queue = new UnmatchedPreviewEventQueue();
    queue.enqueue(origin, previewEvent("preview", "full", 2));
    const reloadError = new Error("reload failed");
    const reports: unknown[] = [];
    const readiness = {
      ...actualReadinessDependencies(runtime, queue),
      reload: async () => Promise.reject(reloadError),
      report: (error: unknown) => reports.push(error),
    };

    await completePreviewReadiness(runtime, preview, readiness);
    assert.equal(preview.ready, false);
    assert.equal(preview.reloadPending, true);
    assert.equal(queue.usage.reloadMarkers, 1);
    assert.deepEqual(reports, [reloadError]);

    let reactivation: Promise<void> | undefined;
    await handlePreviewEventLifecycle(
      origin,
      previewEvent("preview", "full", 3),
      [runtime],
      queue,
      {
        reload: () => assert.fail("full recovery must not reload again"),
        navigate: async () => assert.fail("full event is not navigation"),
        reactivate: (owner, item) => {
          reactivation = beginEmbeddedPreviewHandshake(owner, item, readiness);
          return reactivation;
        },
      },
    );
    assert.ok(reactivation);
    await reactivation;
    assert.deepEqual(publicationTypes, ["full", "full"]);
    assert.equal(preview.ready, true);
  });

  test("keeps readiness false when post fails or membership disappears while pending", async () => {
    for (const outcome of ["false", "reject", "disposed", "closed"] as const) {
      let resolvePost!: (delivered: boolean) => void;
      let rejectPost!: (error: Error) => void;
      const post = new Promise<boolean>((resolve, reject) => {
        resolvePost = resolve;
        rejectPost = reject;
      });
      const rpc = { closed: false } as JsonRpcConnection;
      const origin = daemonOrigin(rpc, "daemon", 1);
      const queue = new UnmatchedPreviewEventQueue();
      const preview = previewState(origin, { postMessage: () => post });
      const runtime = {
        removed: false,
        previews: new Map([["preview", preview]]),
      } as unknown as WorkspaceRuntime;
      let reloads = 0;
      const handshake = beginEmbeddedPreviewHandshake(runtime, preview, {
        ...readinessDependencies(runtime, queue, []),
        reload: async () => {
          reloads += 1;
          return true;
        },
      });
      if (outcome === "disposed") {
        runtime.previews.delete("preview");
        resolvePost(true);
      } else if (outcome === "closed") {
        Object.assign(rpc, { closed: true });
        resolvePost(true);
      } else if (outcome === "reject") rejectPost(new Error("closed"));
      else resolvePost(false);
      await handshake;
      assert.equal(preview.ready, false, outcome);
      assert.equal(
        reloads,
        outcome === "false" || outcome === "reject" ? 1 : 0,
        outcome,
      );
    }
  });

  test("retains a reload marker when actual queued delivery throws and recovers on full", async () => {
    const rpc = { closed: false } as JsonRpcConnection;
    const origin = daemonOrigin(rpc, "daemon", 1);
    const queue = new UnmatchedPreviewEventQueue();
    let failPatch = true;
    const posts: string[] = [];
    const preview = previewState(origin, {
      postMessage: (message: { type: string; event?: { type: string } }) => {
        if (message.type !== "previewEvent") return Promise.resolve(true);
        const type = message.event?.type ?? "missing";
        posts.push(type);
        if (type === "patch" && failPatch)
          return Promise.reject(new Error("post failed"));
        return Promise.resolve(true);
      },
    });
    const runtime = {
      removed: false,
      previews: new Map([["preview", preview]]),
    } as unknown as WorkspaceRuntime;
    queue.enqueue(origin, previewEvent("preview", "full", 2));
    queue.enqueue(origin, previewEvent("preview", "patch", 3));
    queue.enqueue(origin, previewEvent("preview", "viewport", 3));
    let reloads = 0;
    const dependencies = actualReadinessDependencies(runtime, queue, {
      reload: async () => {
        reloads += 1;
        return true;
      },
    });

    await completePreviewReadiness(runtime, preview, dependencies);
    assert.deepEqual(posts, ["full", "patch"]);
    assert.equal(reloads, 1);
    assert.equal(preview.ready, false);
    assert.equal(queue.usage.reloadMarkers, 1);

    failPatch = false;
    let reactivation: Promise<void> | undefined;
    await handlePreviewEventLifecycle(
      origin,
      previewEvent("preview", "full", 4),
      [runtime],
      queue,
      {
        reload: () => assert.fail("full recovery must not reload again"),
        navigate: async () => assert.fail("full event is not navigation"),
        reactivate: (owner, item) => {
          reactivation = beginEmbeddedPreviewHandshake(
            owner,
            item,
            dependencies,
          );
          return reactivation;
        },
      },
    );
    assert.ok(reactivation);
    await reactivation;
    assert.deepEqual(posts, ["full", "patch", "full"]);
    assert.equal(preview.ready, true);
    assert.equal(queue.usage.streams, 0);
  });

  test("reports queued navigation rejection and continues remaining publications in FIFO order", async () => {
    const origin = daemonOrigin(
      { closed: false } as JsonRpcConnection,
      "daemon",
      1,
    );
    const queue = new UnmatchedPreviewEventQueue();
    const publications: string[] = [];
    const preview = previewState(origin, {
      postMessage: (message: { event?: { type: string } }) => {
        publications.push(message.event?.type ?? "missing");
        return Promise.resolve(true);
      },
    });
    const runtime = {
      removed: false,
      previews: new Map([["preview", preview]]),
    } as unknown as WorkspaceRuntime;
    queue.enqueue(origin, sourceNavigationEvent("preview"));
    queue.enqueue(origin, previewEvent("preview", "full", 2));
    queue.enqueue(origin, previewEvent("preview", "patch", 3));
    let reloads = 0;
    const navigationError = new Error("navigation failed");
    const reports: unknown[] = [];
    await completePreviewReadiness(
      runtime,
      preview,
      actualReadinessDependencies(runtime, queue, {
        navigate: async () => {
          throw navigationError;
        },
        reload: () => {
          reloads += 1;
          return true;
        },
        report: (error) => reports.push(error),
      }),
    );
    assert.deepEqual(reports, [navigationError]);
    assert.deepEqual(publications, ["full", "patch"]);
    assert.equal(reloads, 0);
    assert.equal(preview.ready, true);
    assert.equal(queue.usage.streams, 0);
  });

  test("does not let one preview consume queue-wide fail-closed recovery", () => {
    const origin = daemonOrigin(
      { closed: false } as JsonRpcConnection,
      "daemon",
      1,
    );
    const queue = globallyLimitedQueue({ maxStreams: 1 });
    assert.equal(
      queue.enqueue(origin, previewEvent("old", "full", 1)),
      "stored",
    );
    assert.deepEqual(
      queue.enqueue(origin, previewEvent("overflow", "full", 1)),
      { status: "overflow", origins: [origin] },
    );
    assert.equal(queue.usage.failClosed, true);
    assert.equal(
      queue.enqueue(origin, previewEvent("later", "full", 1)),
      "ignored",
    );

    assert.deepEqual(queue.takeForDelivery(origin, "preview-a"), {
      events: [],
      reloadRequired: true,
    });
    assert.deepEqual(queue.takeForDelivery(origin, "preview-b"), {
      events: [],
      reloadRequired: true,
    });
    assert.equal(queue.usage.failClosed, true);
    queue.clearOrigin(origin);
    assert.equal(queue.usage.failClosed, false);
  });

  test("retries checkpoint and create version drift until the latest publication wins", async () => {
    const { runtime } = previewCandidate();
    const state = runtime.documents.get("file:///document.md");
    assert.ok(state);
    const document = {
      languageId: "markdown",
      uri: { toString: () => "file:///document.md" },
      version: 1,
      getText: () => "text",
    };
    const disposed: string[] = [];
    const versions: number[] = [];
    let checkpointCount = 0;
    const rpc = {
      closed: false,
      request(_method: string, params: { expectedDocumentVersion: number }) {
        versions.push(params.expectedDocumentVersion);
        const id = `preview-${params.expectedDocumentVersion}`;
        if (params.expectedDocumentVersion === 2) {
          document.version = 3;
          state.version = 3;
        }
        return Promise.resolve({
          ...previewCandidate().candidate.result,
          previewSessionId: id,
        });
      },
    } as unknown as JsonRpcConnection;
    const origin = daemonOrigin(rpc, "daemon", 1);
    const candidate = await requestPreviewCandidate(
      runtime,
      document as never,
      "embeddedHtml",
      {
        currentOrigin: () => origin,
        checkpoint: async () => {
          checkpointCount += 1;
          if (checkpointCount === 1) {
            document.version = 2;
            state.version = 2;
          }
        },
        identityCurrent: () => true,
        reject: async (item) => {
          disposed.push(item.result.previewSessionId);
        },
      },
    );
    assert.deepEqual(versions, [2, 3]);
    assert.deepEqual(disposed, ["preview-2"]);
    assert.equal(candidate?.documentVersion, 3);
    assert.equal(candidate?.result.previewSessionId, "preview-3");
  });

  test("silently stops after three continuously changing preview candidates", async () => {
    const { runtime } = previewCandidate();
    const state = runtime.documents.get("file:///document.md");
    assert.ok(state);
    const document = {
      uri: { toString: () => "file:///document.md" },
      version: 1,
    };
    const rpc = {
      closed: false,
      request: () => {
        const version = document.version;
        document.version += 1;
        state.version = document.version;
        return Promise.resolve({
          ...previewCandidate().candidate.result,
          previewSessionId: `preview-${version}`,
        });
      },
    } as unknown as JsonRpcConnection;
    const disposed: string[] = [];
    assert.equal(
      await requestPreviewCandidate(
        runtime,
        document as never,
        "embeddedHtml",
        {
          currentOrigin: () => daemonOrigin(rpc, "daemon", 1),
          checkpoint: async () => undefined,
          identityCurrent: () => true,
          reject: async (candidate) => {
            disposed.push(candidate.result.previewSessionId);
          },
        },
      ),
      undefined,
    );
    assert.deepEqual(disposed, ["preview-1", "preview-2", "preview-3"]);
  });

  test("rejects a recreate candidate whose old preview was removed while awaiting", async () => {
    const { candidate, runtime } = previewCandidate();
    const previous = {
      ...runtime.previews.get("preview"),
      documentUri: "file:///document.md",
      previewSessionId: "old-preview",
      target: "externalBrowser",
      originRpc: candidate.origin.rpc,
      originDaemonInstanceId: "daemon",
      originGeneration: 1,
    } as never;
    runtime.previews.set("old-preview", previous);
    const rejected: string[] = [];
    await recreatePreviewsLifecycle(runtime, {
      document: () => ({}) as never,
      request: async () => {
        runtime.previews.delete("old-preview");
        return candidate;
      },
      currentOrigin: () => candidate.origin,
      candidateCurrent: () => true,
      candidateIdentityCurrent: () => true,
      reject: async (item) => {
        rejected.push(item.result.previewSessionId);
      },
      dispose: async () => undefined,
      handshake: async () => undefined,
      activate: async () => undefined,
      discardQueued: () => undefined,
      markReload: () => undefined,
      reload: async () => true,
      openExternal: async () => undefined,
      report: assert.fail,
    });
    assert.deepEqual(rejected, ["preview"]);
    assert.equal(runtime.previews.size, 0);
  });

  test("retains and reloads a recreate when initial delivery fails", async () => {
    for (const failure of ["false", "reject"] as const) {
      const { candidate, runtime } = previewCandidate();
      const previous = {
        documentUri: "file:///document.md",
        previewSessionId: "old-preview",
        target: "embeddedHtml",
        originRpc: candidate.origin.rpc,
        originDaemonInstanceId: "old-daemon",
        originGeneration: 0,
        initialPublication: candidate.result.initialPublication,
        renderRevision: 1,
        panel: {
          webview: {
            postMessage: () =>
              failure === "false"
                ? Promise.resolve(false)
                : Promise.reject(new Error("post failed")),
          },
        },
      } as unknown as PreviewState;
      runtime.previews.set("old-preview", previous);
      let reloaded = 0;
      let disposed = 0;
      const reports: string[] = [];
      const queue = new UnmatchedPreviewEventQueue();
      await recreatePreviewsLifecycle(runtime, {
        document: () => ({}) as never,
        request: async () => candidate,
        currentOrigin: () => candidate.origin,
        candidateCurrent: () => true,
        candidateIdentityCurrent: () => true,
        reject: async () => assert.fail("committed candidate uses disposal"),
        dispose: async (owner, item) => {
          disposed += 1;
          owner.previews.delete(item.previewSessionId);
        },
        handshake: (owner, item) =>
          beginEmbeddedPreviewHandshake(owner, item, {
            ...readinessDependencies(owner, queue, []),
            reload: async () => {
              reloaded += 1;
              return true;
            },
            report: (error) => reports.push(String(error)),
          }),
        activate: async () => assert.fail("failed delivery cannot activate"),
        discardQueued: () => undefined,
        markReload: (origin, previewSessionId) =>
          queue.markReloadRequired(origin, previewSessionId),
        reload: async () => {
          reloaded += 1;
          return true;
        },
        openExternal: async () => assert.fail("embedded preview"),
        report: (error) => reports.push(String(error)),
      });
      assert.equal(reloaded, 1);
      assert.equal(disposed, 0);
      assert.equal(runtime.previews.get("preview"), previous);
      assert.deepEqual(
        reports,
        failure === "reject" ? ["Error: post failed"] : [],
      );
    }
  });

  test("retains a drifted embedded recreate after reload rejection until a full event", async () => {
    const { candidate: fixture, runtime } = previewCandidate();
    const reloadFailure = new Error("embedded reload rejected");
    const rpc = {
      closed: false,
      request: () => Promise.reject(reloadFailure),
    } as unknown as JsonRpcConnection;
    const candidate = {
      ...fixture,
      origin: daemonOrigin(rpc, "daemon", 1),
    } as PreviewCandidate;
    const messages: string[] = [];
    const previous = previewState(candidate.origin, {
      postMessage: (message: unknown) => {
        messages.push((message as { type: string }).type);
        return Promise.resolve(true);
      },
    });
    previous.previewSessionId = "old-preview";
    runtime.previews.set("old-preview", previous);
    const queue = new UnmatchedPreviewEventQueue();
    let versionCurrent = true;
    let activations = 0;
    let disposals = 0;
    const reports: string[] = [];
    const readiness = actualReadinessDependencies(runtime, queue);

    await recreatePreviewsLifecycle(runtime, {
      document: () => ({}) as never,
      request: async () => candidate,
      currentOrigin: () => candidate.origin,
      candidateCurrent: () => versionCurrent,
      candidateIdentityCurrent: () => true,
      reject: async () => assert.fail("committed candidate is retained"),
      dispose: async (owner, item) => {
        disposals += 1;
        owner.previews.delete(item.previewSessionId);
      },
      handshake: async (owner, item) => {
        await beginEmbeddedPreviewHandshake(owner, item, readiness);
        versionCurrent = false;
      },
      activate: async () => {
        activations += 1;
      },
      discardQueued: () => assert.fail("reload rejection is not open failure"),
      markReload: (origin, previewSessionId) =>
        queue.markReloadRequired(origin, previewSessionId),
      reload: (owner, item) =>
        reloadPreviewLifecycle(owner, item, {
          disposed: () => false,
          currentOrigin: () => candidate.origin,
          reportFailure: (message) => reports.push(message),
        }),
      openExternal: async () => assert.fail("embedded preview"),
      report: (error) => reports.push(String(error)),
    });

    assert.equal(runtime.previews.get("preview"), previous);
    assert.equal(previous.ready, false);
    assert.equal(previous.reloadPending, true);
    assert.equal(queue.usage.reloadMarkers, 1);
    assert.equal(activations, 0);
    assert.equal(disposals, 0);
    assert.deepEqual(messages, ["initializePreview"]);
    assert.deepEqual(reports, [
      "preview reload failed: Error: embedded reload rejected",
    ]);

    let reactivation: Promise<void> | undefined;
    await handlePreviewEventLifecycle(
      candidate.origin,
      previewEvent("preview", "full", 2),
      [runtime],
      queue,
      {
        reload: () => assert.fail("full recovery must not reload again"),
        navigate: async () => assert.fail("full event is not navigation"),
        reactivate: (owner, item) => {
          reactivation = beginEmbeddedPreviewHandshake(owner, item, readiness);
          return reactivation;
        },
      },
    );
    assert.ok(reactivation);
    await reactivation;
    assert.deepEqual(messages, ["initializePreview", "previewEvent"]);
    assert.equal(previous.ready, true);
    assert.equal(previous.reloadPending, false);
    assert.equal(queue.usage.streams, 0);
  });

  test("keeps an embedded recreate for replay when its reload marker closes the origin", async () => {
    const { candidate: fixture, runtime } = previewCandidate();
    let closes = 0;
    let reloadRequests = 0;
    const rpcState = {
      closed: false,
      close() {
        closes += 1;
        this.closed = true;
      },
      request() {
        reloadRequests += 1;
        return Promise.reject(new Error("origin closed"));
      },
    };
    const candidate = {
      ...fixture,
      origin: daemonOrigin(
        rpcState as unknown as JsonRpcConnection,
        "daemon",
        1,
      ),
    } as PreviewCandidate;
    const previous = previewState(candidate.origin, {
      postMessage: () => Promise.resolve(true),
    });
    const retainedPanel = previous.panel;
    previous.previewSessionId = "old-preview";
    runtime.previews.set("old-preview", previous);
    const queue = globallyLimitedQueue({ maxStreams: 1 });
    queue.enqueue(candidate.origin, previewEvent("occupied", "full", 1));
    let versionCurrent = true;
    let disposals = 0;
    const reports: string[] = [];

    await recreatePreviewsLifecycle(runtime, {
      document: () => ({}) as never,
      request: async () => candidate,
      currentOrigin: () => candidate.origin,
      candidateCurrent: () => versionCurrent,
      candidateIdentityCurrent: () => !candidate.origin.rpc.closed,
      reject: async () => assert.fail("committed candidate is retained"),
      dispose: async () => {
        disposals += 1;
      },
      handshake: async (owner, item) => {
        await beginEmbeddedPreviewHandshake(
          owner,
          item,
          readinessDependencies(owner, queue, []),
        );
        versionCurrent = false;
      },
      activate: async () => assert.fail("drift requires a full event"),
      discardQueued: () => undefined,
      markReload: (origin, previewSessionId) => {
        const overflow = queue.markReloadRequired(origin, previewSessionId);
        assert.equal(typeof overflow, "object");
        if (typeof overflow === "object")
          for (const affectedOrigin of overflow.origins) {
            affectedOrigin.rpc.close();
            queue.clearOrigin(affectedOrigin);
          }
      },
      reload: (owner, item) =>
        reloadPreviewLifecycle(owner, item, {
          disposed: () => false,
          currentOrigin: () => candidate.origin,
          reportFailure: (message) => reports.push(message),
        }),
      openExternal: async () => assert.fail("embedded preview"),
      report: (error) => reports.push(String(error)),
    });

    assert.equal(closes, 1);
    assert.equal(reloadRequests, 1);
    assert.equal(disposals, 0);
    assert.equal(runtime.previews.get("preview"), previous);
    assert.equal(previous.panel, retainedPanel);
    assert.equal(previous.ready, false);
    assert.equal(previous.reloadPending, true);
    assert.equal(queue.usage.failClosed, false);
    assert.deepEqual(reports, ["preview reload failed: Error: origin closed"]);
  });

  test("keeps an embedded recreate for replay when its origin closes after handshake", async () => {
    const { candidate, runtime } = previewCandidate();
    const previous = previewState(candidate.origin, {
      postMessage: () => Promise.resolve(true),
    });
    const retainedPanel = previous.panel;
    previous.previewSessionId = "old-preview";
    runtime.previews.set("old-preview", previous);
    const queue = new UnmatchedPreviewEventQueue();
    let activations = 0;
    let disposals = 0;

    await recreatePreviewsLifecycle(runtime, {
      document: () => ({}) as never,
      request: async () => candidate,
      currentOrigin: () => candidate.origin,
      candidateCurrent: () => true,
      candidateIdentityCurrent: () => !candidate.origin.rpc.closed,
      reject: async () => assert.fail("committed candidate is retained"),
      dispose: async () => {
        disposals += 1;
      },
      handshake: async (owner, item) => {
        await beginEmbeddedPreviewHandshake(
          owner,
          item,
          readinessDependencies(owner, queue, []),
        );
        Object.assign(candidate.origin.rpc, { closed: true });
      },
      activate: async () => {
        activations += 1;
      },
      discardQueued: () => undefined,
      markReload: () => assert.fail("identity loss is not version drift"),
      reload: async () => assert.fail("identity loss is not version drift"),
      openExternal: async () => assert.fail("embedded preview"),
      report: assert.fail,
    });

    assert.equal(disposals, 0);
    assert.equal(activations, 0);
    assert.equal(runtime.previews.get("preview"), previous);
    assert.equal(previous.panel, retainedPanel);
    assert.equal(previous.ready, true);
  });

  test("ignores an old embedded handshake rejection after the preview is rekeyed", async () => {
    const { candidate, runtime } = previewCandidate();
    const previous = previewState(candidate.origin, {
      postMessage: () => Promise.resolve(true),
    });
    previous.previewSessionId = "old-preview";
    runtime.previews.set("old-preview", previous);
    let observeHandshake!: () => void;
    let rejectHandshake!: (error: Error) => void;
    const handshakeStarted = new Promise<void>((resolve) => {
      observeHandshake = resolve;
    });
    const pendingHandshake = new Promise<void>((_resolve, reject) => {
      rejectHandshake = reject;
    });
    let reloads = 0;
    let disposals = 0;
    const reports: unknown[] = [];

    const recreating = recreatePreviewsLifecycle(runtime, {
      document: () => ({}) as never,
      request: async () => candidate,
      currentOrigin: () => candidate.origin,
      candidateCurrent: () => true,
      candidateIdentityCurrent: () => true,
      reject: async () => assert.fail("committed candidate is retained"),
      dispose: async () => {
        disposals += 1;
      },
      handshake: async () => {
        observeHandshake();
        await pendingHandshake;
      },
      activate: async () => assert.fail("rejected handshake cannot activate"),
      discardQueued: () => undefined,
      markReload: () => undefined,
      reload: async () => {
        reloads += 1;
        return true;
      },
      openExternal: async () => assert.fail("embedded preview"),
      report: (error) => reports.push(error),
    });
    await handshakeStarted;
    const replacementRpc = { closed: false } as JsonRpcConnection;
    runtime.previews.delete("preview");
    previous.previewSessionId = "replacement-preview";
    previous.originRpc = replacementRpc;
    previous.originDaemonInstanceId = "replacement-daemon";
    previous.originGeneration = 2;
    runtime.previews.set("replacement-preview", previous);

    rejectHandshake(new Error("old handshake failed"));
    await recreating;

    assert.equal(reloads, 0);
    assert.equal(disposals, 0);
    assert.deepEqual(reports, []);
    assert.equal(runtime.previews.get("replacement-preview"), previous);
  });

  test("ignores an old embedded activation rejection after same-id origin replacement", async () => {
    const { candidate, runtime } = previewCandidate();
    const previous = previewState(candidate.origin, {
      postMessage: () => Promise.resolve(true),
    });
    previous.previewSessionId = "old-preview";
    runtime.previews.set("old-preview", previous);
    let observeActivation!: () => void;
    let rejectActivation!: (error: Error) => void;
    const activationStarted = new Promise<void>((resolve) => {
      observeActivation = resolve;
    });
    const pendingActivation = new Promise<void>((_resolve, reject) => {
      rejectActivation = reject;
    });
    let disposals = 0;
    const reports: unknown[] = [];

    const recreating = recreatePreviewsLifecycle(runtime, {
      document: () => ({}) as never,
      request: async () => candidate,
      currentOrigin: () => candidate.origin,
      candidateCurrent: () => true,
      candidateIdentityCurrent: () => true,
      reject: async () => assert.fail("committed candidate is retained"),
      dispose: async () => {
        disposals += 1;
      },
      handshake: async (_owner, item) => {
        item.ready = true;
      },
      activate: async () => {
        observeActivation();
        await pendingActivation;
      },
      discardQueued: () => undefined,
      markReload: () => undefined,
      reload: async () => true,
      openExternal: async () => assert.fail("embedded preview"),
      report: (error) => reports.push(error),
    });
    await activationStarted;
    const replacementRpc = { closed: false } as JsonRpcConnection;
    previous.originRpc = replacementRpc;
    previous.originDaemonInstanceId = "replacement-daemon";
    previous.originGeneration = 2;
    previous.ready = false;

    rejectActivation(new Error("old activation failed"));
    await recreating;

    assert.equal(disposals, 0);
    assert.deepEqual(reports, []);
    assert.equal(runtime.previews.get("preview"), previous);
    assert.equal(previous.originRpc, replacementRpc);
    assert.equal(previous.ready, false);
  });

  test("retains a drifted external recreate after reload rejection until a full event", async () => {
    const { candidate: fixture, runtime } = previewCandidate();
    const reloadFailure = new Error("external reload rejected");
    const rpc = {
      closed: false,
      request: () => Promise.reject(reloadFailure),
    } as unknown as JsonRpcConnection;
    const candidate = {
      ...fixture,
      origin: daemonOrigin(rpc, "daemon", 1),
    } as PreviewCandidate;
    const previous = {
      documentUri: "file:///document.md",
      previewSessionId: "old-preview",
      target: "externalBrowser",
      originRpc: candidate.origin.rpc,
      originDaemonInstanceId: candidate.origin.daemonInstanceId,
      originGeneration: candidate.origin.generation,
      initialPublication: candidate.result.initialPublication,
      renderRevision: 1,
    } as PreviewState;
    runtime.previews.set("old-preview", previous);
    const queue = new UnmatchedPreviewEventQueue();
    let versionCurrent = true;
    let activations = 0;
    let disposals = 0;
    const reports: string[] = [];
    const activate = async (owner: WorkspaceRuntime, item: PreviewState) => {
      activations += 1;
      await completePreviewReadiness(
        owner,
        item,
        actualReadinessDependencies(owner, queue),
      );
    };

    await recreatePreviewsLifecycle(runtime, {
      document: () => ({}) as never,
      request: async () => candidate,
      currentOrigin: () => candidate.origin,
      candidateCurrent: () => versionCurrent,
      candidateIdentityCurrent: () => true,
      reject: async () => assert.fail("committed candidate is retained"),
      dispose: async (owner, item) => {
        disposals += 1;
        owner.previews.delete(item.previewSessionId);
      },
      handshake: async () => assert.fail("external preview has no handshake"),
      activate,
      discardQueued: () => assert.fail("reload rejection is not open failure"),
      markReload: (origin, previewSessionId) =>
        queue.markReloadRequired(origin, previewSessionId),
      reload: (owner, item) =>
        reloadPreviewLifecycle(owner, item, {
          disposed: () => false,
          currentOrigin: () => candidate.origin,
          reportFailure: (message) => reports.push(message),
        }),
      openExternal: async () => {
        versionCurrent = false;
      },
      report: (error) => reports.push(String(error)),
    });

    assert.equal(runtime.previews.get("preview"), previous);
    assert.equal(previous.ready, false);
    assert.equal(previous.reloadPending, true);
    assert.equal(queue.usage.reloadMarkers, 1);
    assert.equal(activations, 0);
    assert.equal(disposals, 0);
    assert.deepEqual(reports, [
      "preview reload failed: Error: external reload rejected",
    ]);

    let reactivation: Promise<void> | undefined;
    await handlePreviewEventLifecycle(
      candidate.origin,
      previewEvent("preview", "full", 2),
      [runtime],
      queue,
      {
        reload: () => assert.fail("full recovery must not reload again"),
        navigate: async () => assert.fail("full event is not navigation"),
        reactivate: (owner, item) => {
          reactivation = activate(owner, item);
          return reactivation;
        },
      },
    );
    assert.ok(reactivation);
    await reactivation;
    assert.equal(activations, 1);
    assert.equal(previous.ready, true);
    assert.equal(previous.reloadPending, false);
    assert.equal(queue.usage.streams, 0);
  });

  test("keeps an external recreate for replay when its origin closes during URL open", async () => {
    const { candidate, runtime } = previewCandidate();
    const previous = {
      documentUri: "file:///document.md",
      previewSessionId: "old-preview",
      target: "externalBrowser",
      originRpc: candidate.origin.rpc,
      originDaemonInstanceId: candidate.origin.daemonInstanceId,
      originGeneration: candidate.origin.generation,
      initialPublication: candidate.result.initialPublication,
      renderRevision: 1,
    } as PreviewState;
    runtime.previews.set("old-preview", previous);
    let activations = 0;
    let disposals = 0;

    await recreatePreviewsLifecycle(runtime, {
      document: () => ({}) as never,
      request: async () => candidate,
      currentOrigin: () => candidate.origin,
      candidateCurrent: () => true,
      candidateIdentityCurrent: () => !candidate.origin.rpc.closed,
      reject: async () => assert.fail("committed candidate is retained"),
      dispose: async () => {
        disposals += 1;
      },
      handshake: async () => assert.fail("external preview has no handshake"),
      activate: async () => {
        activations += 1;
      },
      discardQueued: () => undefined,
      markReload: () => assert.fail("identity loss is not version drift"),
      reload: async () => assert.fail("identity loss is not version drift"),
      openExternal: async () => {
        Object.assign(candidate.origin.rpc, { closed: true });
      },
      report: assert.fail,
    });

    assert.equal(disposals, 0);
    assert.equal(activations, 0);
    assert.equal(runtime.previews.get("preview"), previous);
    assert.equal(previous.ready, false);
  });

  test("does not mark a same-id replacement ready when old external recreation rejects", async () => {
    const { candidate, runtime } = previewCandidate();
    const previous = {
      documentUri: "file:///document.md",
      previewSessionId: "old-preview",
      target: "externalBrowser",
      originRpc: candidate.origin.rpc,
      originDaemonInstanceId: candidate.origin.daemonInstanceId,
      originGeneration: candidate.origin.generation,
      initialPublication: candidate.result.initialPublication,
      renderRevision: 1,
    } as PreviewState;
    runtime.previews.set("old-preview", previous);
    let observeOpen!: () => void;
    let rejectOpen!: (error: Error) => void;
    const opened = new Promise<void>((resolve) => {
      observeOpen = resolve;
    });
    const pendingOpen = new Promise<void>((_resolve, reject) => {
      rejectOpen = reject;
    });
    const discarded: string[] = [];
    const reports: unknown[] = [];

    const recreating = recreatePreviewsLifecycle(runtime, {
      document: () => ({}) as never,
      request: async () => candidate,
      currentOrigin: () => candidate.origin,
      candidateCurrent: () => true,
      candidateIdentityCurrent: () => true,
      reject: async () => assert.fail("committed candidate is retained"),
      dispose: async () => assert.fail("external failure retains membership"),
      handshake: async () => assert.fail("external preview has no handshake"),
      activate: async () => assert.fail("rejected URL open cannot activate"),
      discardQueued: (_origin, previewSessionId) => {
        discarded.push(previewSessionId);
      },
      markReload: () => undefined,
      reload: async () => true,
      openExternal: async () => {
        observeOpen();
        await pendingOpen;
      },
      report: (error) => reports.push(error),
    });
    await opened;
    const replacementRpc = { closed: false } as JsonRpcConnection;
    previous.originRpc = replacementRpc;
    previous.originDaemonInstanceId = "replacement-daemon";
    previous.originGeneration = 2;
    previous.ready = false;

    const failure = new Error("old browser open failed");
    rejectOpen(failure);
    await recreating;

    assert.deepEqual(discarded, ["preview"]);
    assert.deepEqual(reports, []);
    assert.equal(runtime.previews.get("preview"), previous);
    assert.equal(previous.originRpc, replacementRpc);
    assert.equal(previous.ready, false);
  });

  test("reports a current external recreation rejection and restores readiness", async () => {
    const { candidate, runtime } = previewCandidate();
    const previous = {
      documentUri: "file:///document.md",
      previewSessionId: "old-preview",
      target: "externalBrowser",
      originRpc: candidate.origin.rpc,
      originDaemonInstanceId: candidate.origin.daemonInstanceId,
      originGeneration: candidate.origin.generation,
      initialPublication: candidate.result.initialPublication,
      renderRevision: 1,
    } as PreviewState;
    runtime.previews.set("old-preview", previous);
    const failure = new Error("current browser open failed");
    const reports: unknown[] = [];

    await recreatePreviewsLifecycle(runtime, {
      document: () => ({}) as never,
      request: async () => candidate,
      currentOrigin: () => candidate.origin,
      candidateCurrent: () => true,
      candidateIdentityCurrent: () => true,
      reject: async () => assert.fail("committed candidate is retained"),
      dispose: async () => assert.fail("external failure retains membership"),
      handshake: async () => assert.fail("external preview has no handshake"),
      activate: async () => assert.fail("rejected URL open cannot activate"),
      discardQueued: () => undefined,
      markReload: () => undefined,
      reload: async () => true,
      openExternal: async () => {
        throw failure;
      },
      report: (error) => reports.push(error),
    });

    assert.deepEqual(reports, [failure]);
    assert.equal(runtime.previews.get("preview"), previous);
    assert.equal(previous.ready, true);
  });

  test("retires the old handshake before recreate mutation and ignores its completion", async () => {
    const { candidate, runtime } = previewCandidate();
    const oldOrigin = daemonOrigin(
      { closed: false } as JsonRpcConnection,
      "old-daemon",
      0,
    );
    let resolveOldPost!: (delivered: boolean) => void;
    const oldPost = new Promise<boolean>((resolve) => {
      resolveOldPost = resolve;
    });
    const posts: string[] = [];
    const previous = previewState(oldOrigin, {
      postMessage: (message: { publication: { previewSessionId: string } }) => {
        posts.push(message.publication.previewSessionId);
        return posts.length === 1 ? oldPost : Promise.resolve(true);
      },
    });
    previous.previewSessionId = "old-preview";
    previous.initialPublication = {
      ...previous.initialPublication,
      previewSessionId: "old-preview",
    };
    runtime.previews.set("old-preview", previous);
    const queue = new UnmatchedPreviewEventQueue();
    let reloads = 0;
    let disposals = 0;
    let activations = 0;
    const reports: unknown[] = [];
    const readiness = {
      ...readinessDependencies(runtime, queue, []),
      reload: async () => {
        reloads += 1;
        return true;
      },
      report: (error: unknown) => reports.push(error),
    };
    const oldHandshake = beginEmbeddedPreviewHandshake(
      runtime,
      previous,
      readiness,
    );
    await Promise.resolve();
    assert.deepEqual(posts, ["old-preview"]);

    await recreatePreviewsLifecycle(runtime, {
      document: () => ({}) as never,
      request: async () => candidate,
      currentOrigin: () => candidate.origin,
      candidateCurrent: () => true,
      candidateIdentityCurrent: () => true,
      reject: async () => assert.fail("candidate is committed"),
      dispose: async (owner, item) => {
        disposals += 1;
        owner.previews.delete(item.previewSessionId);
      },
      handshake: (owner, item) =>
        beginEmbeddedPreviewHandshake(owner, item, readiness),
      activate: async () => {
        activations += 1;
      },
      discardQueued: () => undefined,
      markReload: () => undefined,
      reload: async () => {
        reloads += 1;
        return true;
      },
      openExternal: async () => assert.fail("embedded preview"),
      report: (error) => reports.push(error),
    });
    const completedEpoch = previous.handshakeEpoch;
    assert.deepEqual(posts, ["old-preview", "preview"]);
    assert.equal(previous.previewSessionId, "preview");
    assert.equal(previous.ready, true);
    assert.equal(activations, 1);

    resolveOldPost(false);
    await oldHandshake;
    assert.equal(previous.handshakeEpoch, completedEpoch);
    assert.equal(previous.ready, true);
    assert.equal(reloads, 0);
    assert.equal(disposals, 0);
    assert.deepEqual(reports, []);
  });

  test("removes stale-origin previews on every precommit recreate failure", async () => {
    for (const failure of [
      "undefined",
      "throw",
      "missing-url",
      "stale-candidate",
    ] as const) {
      const { candidate, runtime } = previewCandidate();
      const oldOrigin = daemonOrigin(
        { closed: false } as JsonRpcConnection,
        "old-daemon",
        0,
      );
      const previous = {
        documentUri: "file:///document.md",
        previewSessionId: "old-preview",
        target: "externalBrowser",
        originRpc: oldOrigin.rpc,
        originDaemonInstanceId: oldOrigin.daemonInstanceId,
        originGeneration: oldOrigin.generation,
        initialPublication: candidate.result.initialPublication,
        renderRevision: 1,
      } as PreviewState;
      runtime.previews.set("old-preview", previous);
      let disposals = 0;
      let rejections = 0;
      const reports: unknown[] = [];
      const requestFailure = new Error("recreate request failed");
      const missingUrl = {
        ...candidate,
        result: { ...candidate.result, url: undefined },
      };
      await recreatePreviewsLifecycle(runtime, {
        document: () => ({}) as never,
        request: async () => {
          if (failure === "throw") throw requestFailure;
          if (failure === "missing-url") return missingUrl;
          if (failure === "stale-candidate") return candidate;
          return undefined;
        },
        currentOrigin: () => candidate.origin,
        candidateCurrent: () => failure !== "stale-candidate",
        candidateIdentityCurrent: () => true,
        reject: async () => {
          rejections += 1;
        },
        dispose: async (owner, item) => {
          disposals += 1;
          owner.previews.delete(item.previewSessionId);
        },
        handshake: async () => assert.fail("precommit failure"),
        activate: async () => assert.fail("precommit failure"),
        discardQueued: () => undefined,
        markReload: () => assert.fail("precommit failure"),
        reload: async () => assert.fail("precommit failure"),
        openExternal: async () => assert.fail("precommit failure"),
        report: (error) => reports.push(error),
      });

      assert.equal(disposals, 1, failure);
      assert.equal(runtime.previews.size, 0, failure);
      assert.equal(
        rejections,
        failure === "missing-url" || failure === "stale-candidate" ? 1 : 0,
        failure,
      );
      assert.equal(
        reports.length,
        failure === "throw" || failure === "missing-url" ? 1 : 0,
        failure,
      );
    }
  });

  test("keeps an old preview for replay when no replacement origin exists", async () => {
    for (const outcome of ["undefined", "throw"] as const) {
      const { candidate, runtime } = previewCandidate();
      const previous = {
        documentUri: "file:///document.md",
        previewSessionId: "old-preview",
        target: "externalBrowser",
        originRpc: candidate.origin.rpc,
        originDaemonInstanceId: candidate.origin.daemonInstanceId,
        originGeneration: candidate.origin.generation,
        initialPublication: candidate.result.initialPublication,
        renderRevision: 1,
      } as PreviewState;
      runtime.previews.set("old-preview", previous);
      let disposals = 0;
      const reports: unknown[] = [];
      const failure = new Error("origin closed during recovery");

      await recreatePreviewsLifecycle(runtime, {
        document: () => ({}) as never,
        request: async () => {
          if (outcome === "throw") throw failure;
          return undefined;
        },
        currentOrigin: () => undefined,
        candidateCurrent: () => assert.fail("there is no candidate"),
        candidateIdentityCurrent: () => assert.fail("there is no candidate"),
        reject: async () => assert.fail("there is no candidate"),
        dispose: async () => {
          disposals += 1;
        },
        handshake: async () => assert.fail("there is no candidate"),
        activate: async () => assert.fail("there is no candidate"),
        discardQueued: () => assert.fail("there is no candidate"),
        markReload: () => assert.fail("there is no candidate"),
        reload: async () => assert.fail("there is no candidate"),
        openExternal: async () => assert.fail("there is no candidate"),
        report: (error) => reports.push(error),
      });

      assert.equal(disposals, 0, outcome);
      assert.equal(runtime.previews.get("old-preview"), previous, outcome);
      assert.deepEqual(reports, outcome === "throw" ? [failure] : [], outcome);
    }
  });

  test("does not activate a recreated external preview removed while URL opening is pending", async () => {
    const { candidate, runtime } = previewCandidate();
    const previous = {
      documentUri: "file:///document.md",
      previewSessionId: "old-preview",
      target: "externalBrowser",
      originRpc: candidate.origin.rpc,
      originDaemonInstanceId: "old-daemon",
      originGeneration: 0,
      initialPublication: candidate.result.initialPublication,
      renderRevision: 1,
    } as PreviewState;
    runtime.previews.set("old-preview", previous);
    let releaseOpen!: () => void;
    let observedOpen!: () => void;
    const pendingOpen = new Promise<void>((resolve) => {
      releaseOpen = resolve;
    });
    const opened = new Promise<void>((resolve) => {
      observedOpen = resolve;
    });
    let activations = 0;
    const recreating = recreatePreviewsLifecycle(runtime, {
      document: () => ({}) as never,
      request: async () => candidate,
      currentOrigin: () => candidate.origin,
      candidateCurrent: () => true,
      candidateIdentityCurrent: () => true,
      reject: async () => undefined,
      dispose: async () => undefined,
      handshake: async () => undefined,
      activate: async () => {
        activations += 1;
      },
      discardQueued: () => undefined,
      markReload: () => undefined,
      reload: async () => true,
      openExternal: async () => {
        observedOpen();
        await pendingOpen;
      },
      report: assert.fail,
    });
    await opened;
    runtime.removed = true;
    runtime.previews.clear();
    releaseOpen();
    await recreating;
    assert.equal(activations, 0);
  });

  test("reloads a drifted recreated external preview and waits for an authoritative full event", async () => {
    const { candidate, runtime } = previewCandidate();
    const previous = {
      documentUri: "file:///document.md",
      previewSessionId: "old-preview",
      target: "externalBrowser",
      originRpc: candidate.origin.rpc,
      originDaemonInstanceId: candidate.origin.daemonInstanceId,
      originGeneration: candidate.origin.generation,
      initialPublication: candidate.result.initialPublication,
      renderRevision: 1,
    } as PreviewState;
    runtime.previews.set("old-preview", previous);
    let versionCurrent = true;
    const effects: string[] = [];
    const queue = new UnmatchedPreviewEventQueue();
    const activate = async (owner: WorkspaceRuntime, item: PreviewState) => {
      effects.push("activate");
      await completePreviewReadiness(
        owner,
        item,
        actualReadinessDependencies(owner, queue),
      );
    };

    await recreatePreviewsLifecycle(runtime, {
      document: () => ({}) as never,
      request: async () => candidate,
      currentOrigin: () => candidate.origin,
      candidateCurrent: () => versionCurrent,
      candidateIdentityCurrent: () => true,
      reject: async () => assert.fail("current candidate is retained"),
      dispose: async () => assert.fail("current candidate is retained"),
      handshake: async () => assert.fail("external preview has no handshake"),
      activate,
      discardQueued: () => undefined,
      markReload: (origin, previewSessionId) =>
        queue.markReloadRequired(origin, previewSessionId),
      reload: async () => {
        effects.push("reload");
        return true;
      },
      openExternal: async () => {
        effects.push("open");
        versionCurrent = false;
      },
      report: assert.fail,
    });

    assert.deepEqual(effects, ["open", "reload"]);
    assert.equal(runtime.previews.get("preview"), previous);
    assert.equal(previous.ready, false);
    assert.equal(previous.reloadPending, true);
    assert.equal(queue.usage.reloadMarkers, 1);

    let reactivation: Promise<void> | undefined;
    await handlePreviewEventLifecycle(
      candidate.origin,
      previewEvent("preview", "full", 2),
      [runtime],
      queue,
      {
        reload: () => assert.fail("full recovery must not reload again"),
        navigate: async () => assert.fail("full event is not navigation"),
        reactivate: (owner, item) => {
          reactivation = activate(owner, item);
          return reactivation;
        },
      },
    );
    assert.ok(reactivation);
    await reactivation;
    assert.deepEqual(effects, ["open", "reload", "activate"]);
    assert.equal(previous.ready, true);
    assert.equal(previous.reloadPending, false);
  });

  test("deletes membership before awaiting disposal and disposes the panel exactly once", async () => {
    let resolve!: () => void;
    const pending = new Promise<void>((done) => {
      resolve = done;
    });
    const order: string[] = [];
    const rpc = {
      request: () => {
        order.push("rpc");
        return pending;
      },
    } as unknown as JsonRpcConnection;
    const preview = {
      originRpc: rpc,
      originDaemonInstanceId: "daemon",
      originGeneration: 1,
      previewSessionId: "preview",
      panel: { dispose: () => order.push("panel") },
    } as never;
    const runtime = {
      previews: new Map([["preview", preview]]),
    } as unknown as WorkspaceRuntime;
    const disposing = disposePreviewLifecycle(runtime, preview, {
      clearQueued: () => order.push("queue"),
      reportFailure: assert.fail,
    });
    assert.equal(runtime.previews.size, 0);
    assert.deepEqual(order, ["queue", "rpc"]);
    await disposePreviewLifecycle(runtime, preview, {
      clearQueued: assert.fail,
      reportFailure: assert.fail,
    });
    resolve();
    await disposing;
    assert.deepEqual(order, ["queue", "rpc", "panel"]);
  });

  test("reports a rejected disposal but still closes the panel", async () => {
    const order: string[] = [];
    const rpc = {
      request: () => Promise.reject(new Error("closed")),
    } as unknown as JsonRpcConnection;
    const preview = {
      originRpc: rpc,
      originDaemonInstanceId: "daemon",
      originGeneration: 1,
      previewSessionId: "preview",
      panel: { dispose: () => order.push("panel") },
    } as unknown as PreviewState;
    const runtime = {
      previews: new Map([["preview", preview]]),
    } as unknown as WorkspaceRuntime;
    await disposePreviewLifecycle(runtime, preview, {
      clearQueued: () => order.push("queue"),
      reportFailure: (message) => order.push(message),
    });
    assert.equal(runtime.previews.size, 0);
    assert.deepEqual(order, [
      "queue",
      "preview disposal failed: Error: closed",
      "panel",
    ]);
  });

  test("returns false and reports when the current preview reload RPC rejects", async () => {
    const calls: unknown[] = [];
    const reloadError = new Error("reload rejected");
    const rpc = {
      closed: false,
      request(method: string, params: unknown) {
        calls.push({ method, params });
        return Promise.reject(reloadError);
      },
    } as unknown as JsonRpcConnection;
    const origin = daemonOrigin(rpc, "daemon", 4);
    const preview = previewState(origin, {
      postMessage: async () => true,
    });
    const runtime = {
      removed: false,
      previews: new Map([["preview", preview]]),
    } as unknown as WorkspaceRuntime;
    const reports: string[] = [];

    assert.equal(
      await reloadPreviewLifecycle(runtime, preview, {
        disposed: () => false,
        currentOrigin: () => origin,
        reportFailure: (message) => reports.push(message),
      }),
      false,
    );
    assert.deepEqual(calls, [
      {
        method: "fleximark/reloadPreview",
        params: {
          daemonInstanceId: "daemon",
          previewSessionId: "preview",
        },
      },
    ]);
    assert.deepEqual(reports, [
      "preview reload failed: Error: reload rejected",
    ]);
  });

  test("suppresses a late reload result and error after same-id origin replacement", async () => {
    for (const outcome of ["resolve", "reject"] as const) {
      let settleRequest!: () => void;
      const pendingRequest = new Promise<void>((resolve, reject) => {
        settleRequest =
          outcome === "resolve" ? resolve : () => reject(new Error("stale"));
      });
      const rpc = {
        closed: false,
        request: () => pendingRequest,
      } as unknown as JsonRpcConnection;
      const origin = daemonOrigin(rpc, "daemon", 4);
      const preview = previewState(origin, {
        postMessage: async () => true,
      });
      const runtime = {
        removed: false,
        previews: new Map([["preview", preview]]),
      } as unknown as WorkspaceRuntime;
      const reports: string[] = [];

      const reloading = reloadPreviewLifecycle(runtime, preview, {
        disposed: () => false,
        currentOrigin: () => origin,
        reportFailure: (message) => reports.push(message),
      });
      const replacementRpc = { closed: false } as JsonRpcConnection;
      preview.originRpc = replacementRpc;
      preview.originDaemonInstanceId = "replacement-daemon";
      preview.originGeneration = 5;
      settleRequest();

      assert.equal(await reloading, false, outcome);
      assert.deepEqual(reports, [], outcome);
      assert.equal(runtime.previews.get("preview"), preview, outcome);
      assert.equal(preview.originRpc, replacementRpc, outcome);
    }
  });

  test("suppresses navigation effects when any awaited preview identity dimension goes stale", async () => {
    for (const stale of [
      "membership",
      "removed",
      "generation",
      "revision",
      "version",
      "editor",
    ] as const) {
      const rpc = {} as JsonRpcConnection;
      const document = { version: 1 } as { version: number };
      const preview = {
        originRpc: rpc,
        originDaemonInstanceId: "daemon",
        originGeneration: 1,
        documentUri: "file:///document.md",
        previewSessionId: "preview",
        renderRevision: 1,
      } as PreviewState;
      const runtime = {
        removed: false,
        previews: new Map([["preview", preview]]),
      } as unknown as WorkspaceRuntime;
      let resolveEditor!: (editor: unknown) => void;
      const shown = new Promise((resolve) => {
        resolveEditor = resolve;
      });
      const effects: string[] = [];
      const navigating = applySourceNavigationLifecycle(
        runtime,
        preview,
        {
          type: "selectSource",
          sourceRange: {
            start: { line: 0, character: 0, encoding: "utf16" },
            end: { line: 0, character: 1, encoding: "utf16" },
            byteStart: 0,
            byteEnd: 1,
          },
        },
        {
          document: () => document as never,
          sourcePositionInDocument: () => true,
          range: () => ({}) as never,
          visibleEditor: () => undefined,
          showEditor: () => shown as never,
          current: (owner, item) =>
            !owner.removed && owner.previews.get("preview") === item,
          documentOpen: () => true,
          selection: () => ({}) as never,
          select: () => effects.push("select"),
          reveal: () => effects.push("reveal"),
          setSelectionEcho: () => ({}),
          clearSelectionEcho: () => undefined,
          setViewportEcho: () => ({}),
          clearViewportEcho: () => undefined,
          schedule: () => effects.push("timer"),
        },
      );
      let editorDocument: unknown = document;
      if (stale === "membership") runtime.previews.delete("preview");
      if (stale === "removed") runtime.removed = true;
      if (stale === "generation") preview.originGeneration = 2;
      if (stale === "revision") preview.renderRevision = 2;
      if (stale === "version") document.version = 2;
      if (stale === "editor") editorDocument = {};
      resolveEditor({ document: editorDocument });
      await navigating;
      assert.deepEqual(effects, [], stale);
    }
  });

  test("suppresses exact navigation echoes and forwards non-echo editor events", () => {
    const calls: { method: string; params: unknown }[] = [];
    const rpc = {
      notify(method: string, params: unknown) {
        calls.push({ method, params });
      },
    } as unknown as JsonRpcConnection;
    const uri = "file:///document.md";
    const document = { uri: { toString: () => uri }, version: 4 };
    const runtime = {
      documents: new Map([[uri, { sessionId: "session", version: 4 }]]),
    } as unknown as WorkspaceRuntime;
    const dependencies = {
      runtime: () => runtime,
      currentOrigin: () => daemonOrigin(rpc, "daemon", 1),
    };
    const echoes: PreviewEchoState = {
      selection: { uri, value: "0:1-0:2" },
      viewport: { uri },
    };
    handleEditorSelectionLifecycle(
      {
        textEditor: { document },
        selections: [
          {
            anchor: { line: 0, character: 1 },
            active: { line: 0, character: 2 },
          },
        ],
      } as never,
      echoes,
      dependencies,
    );
    handleEditorViewportLifecycle(
      { textEditor: { document }, visibleRanges: [] } as never,
      echoes,
      dependencies,
    );
    assert.deepEqual(calls, []);
    assert.equal(echoes.selection, undefined);
    assert.equal(echoes.viewport, undefined);

    handleEditorSelectionLifecycle(
      {
        textEditor: { document },
        selections: [
          {
            anchor: { line: 1, character: 0 },
            active: { line: 1, character: 3 },
          },
        ],
      } as never,
      echoes,
      dependencies,
    );
    handleEditorViewportLifecycle(
      {
        textEditor: { document },
        visibleRanges: [
          { start: { line: 2, character: 1 }, end: { line: 5, character: 0 } },
        ],
      } as never,
      echoes,
      dependencies,
    );
    assert.deepEqual(calls, [
      {
        method: "fleximark/setSelection",
        params: {
          daemonInstanceId: "daemon",
          documentSessionId: "session",
          expectedDocumentVersion: 4,
          selections: [
            {
              anchor: { line: 1, character: 0 },
              active: { line: 1, character: 3 },
            },
          ],
        },
      },
      {
        method: "fleximark/setViewport",
        params: {
          daemonInstanceId: "daemon",
          documentSessionId: "session",
          expectedDocumentVersion: 4,
          ranges: [
            {
              start: { line: 2, character: 1 },
              end: { line: 5, character: 0 },
            },
          ],
        },
      },
    ]);
  });

  test("queues full, patch, and navigation in FIFO order per exact origin", () => {
    const rpc = {} as JsonRpcConnection;
    const origin = daemonOrigin(rpc, "daemon", 1);
    const queue = new UnmatchedPreviewEventQueue();
    const events = [
      previewEvent("preview", "full", 1),
      previewEvent("preview", "patch", 2),
      previewEvent("preview", "viewport", 2),
    ];
    for (const event of events) queue.enqueue(origin, event);

    assert.deepEqual(queue.take(origin, "preview"), {
      events,
      reloadRequired: false,
    });
    assert.equal(queue.take(origin, "preview"), undefined);
  });

  test("separates the same preview id across rpc origins", () => {
    const firstOrigin = daemonOrigin({} as JsonRpcConnection, "daemon", 1);
    const secondOrigin = daemonOrigin({} as JsonRpcConnection, "daemon", 1);
    const queue = new UnmatchedPreviewEventQueue();
    const first = previewEvent("shared", "full", 1);
    const second = previewEvent("shared", "full", 9);
    queue.enqueue(firstOrigin, first);
    queue.enqueue(secondOrigin, second);

    assert.deepEqual(queue.take(secondOrigin, "shared"), {
      events: [second],
      reloadRequired: false,
    });
    assert.deepEqual(queue.take(firstOrigin, "shared"), {
      events: [first],
      reloadRequired: false,
    });
  });

  test("separates the same preview id across daemon instances", () => {
    const rpc = {} as JsonRpcConnection;
    const firstOrigin = daemonOrigin(rpc, "daemon-first", 1);
    const secondOrigin = daemonOrigin(rpc, "daemon-second", 1);
    const queue = new UnmatchedPreviewEventQueue();
    const first = previewEvent("shared", "full", 1, "daemon-first");
    const second = previewEvent("shared", "full", 9, "daemon-second");
    queue.enqueue(firstOrigin, first);
    queue.enqueue(secondOrigin, second);

    assert.deepEqual(queue.take(firstOrigin, "shared"), {
      events: [first],
      reloadRequired: false,
    });
    assert.deepEqual(queue.take(secondOrigin, "shared"), {
      events: [second],
      reloadRequired: false,
    });
  });

  test("separates a reused preview id across connection generations", () => {
    const rpc = {} as JsonRpcConnection;
    const oldOrigin = daemonOrigin(rpc, "daemon", 3);
    const newOrigin = daemonOrigin(rpc, "daemon", 4);
    const queue = new UnmatchedPreviewEventQueue();
    const oldEvent = previewEvent("reused", "patch", 2);
    const newEvent = previewEvent("reused", "full", 1);
    queue.enqueue(oldOrigin, oldEvent);
    queue.enqueue(newOrigin, newEvent);

    assert.deepEqual(queue.take(newOrigin, "reused"), {
      events: [newEvent],
      reloadRequired: false,
    });
    assert.deepEqual(queue.take(oldOrigin, "reused"), {
      events: [oldEvent],
      reloadRequired: false,
    });
  });

  test("marks a count-overflowed stream as reload-required", () => {
    const origin = daemonOrigin({} as JsonRpcConnection, "daemon", 1);
    const queue = new UnmatchedPreviewEventQueue({
      maxEventsPerPreview: 2,
      maxBytesPerPreview: 100_000,
    });
    queue.enqueue(origin, previewEvent("preview", "full", 1));
    queue.enqueue(origin, previewEvent("preview", "patch", 2));
    queue.enqueue(origin, previewEvent("preview", "viewport", 2));

    assert.deepEqual(queue.take(origin, "preview"), {
      events: [],
      reloadRequired: true,
    });
  });

  test("marks a single oversized event as reload-required", () => {
    const origin = daemonOrigin({} as JsonRpcConnection, "daemon", 1);
    const event = previewEvent("preview", "full", 1);
    const queue = new UnmatchedPreviewEventQueue({
      maxEventsPerPreview: 64,
      maxBytesPerPreview: conservativePreviewEventBytes(event) - 1,
    });
    queue.enqueue(origin, event);

    assert.deepEqual(queue.take(origin, "preview"), {
      events: [],
      reloadRequired: true,
    });
  });

  test("marks a stream whose conservative byte total overflows", () => {
    const origin = daemonOrigin({} as JsonRpcConnection, "daemon", 1);
    const first = previewEvent("preview", "full", 1);
    const second = previewEvent("preview", "patch", 2);
    const queue = new UnmatchedPreviewEventQueue({
      maxEventsPerPreview: 64,
      maxBytesPerPreview:
        conservativePreviewEventBytes(first) +
        conservativePreviewEventBytes(second) -
        1,
    });
    queue.enqueue(origin, first);
    queue.enqueue(origin, second);

    assert.deepEqual(queue.take(origin, "preview"), {
      events: [],
      reloadRequired: true,
    });
  });

  test("clears one exact origin or all origins", () => {
    const rpc = {} as JsonRpcConnection;
    const firstOrigin = daemonOrigin(rpc, "daemon", 1);
    const secondOrigin = daemonOrigin(rpc, "daemon", 2);
    const queue = new UnmatchedPreviewEventQueue();
    queue.enqueue(firstOrigin, previewEvent("first", "full", 1));
    queue.enqueue(secondOrigin, previewEvent("second", "full", 1));
    queue.clearOrigin(firstOrigin);
    assert.equal(queue.take(firstOrigin, "first"), undefined);
    assert.notEqual(queue.take(secondOrigin, "second"), undefined);

    queue.enqueue(firstOrigin, previewEvent("first", "full", 1));
    queue.enqueue(secondOrigin, previewEvent("second", "full", 1));
    queue.clearAll();
    assert.equal(queue.take(firstOrigin, "first"), undefined);
    assert.equal(queue.take(secondOrigin, "second"), undefined);
  });

  test("measures the exact UTF-8 JSON bytes", () => {
    const event = previewEvent("日本語-😀", "full", 1);
    if (event.event.type !== "full") assert.fail("full event expected");
    event.event.html = "é-日本語-😀-\u0000-\n-\t";
    assert.equal(
      previewEventUtf8Bytes(event),
      Buffer.byteLength(JSON.stringify(event), "utf8"),
    );
  });

  test("rejects every invalid local or global queue limit", () => {
    const valid = {
      maxEventsPerPreview: 2,
      maxBytesPerPreview: 2,
      maxStreams: 2,
      maxTotalEvents: 2,
      maxTotalBytes: 2,
    };
    for (const key of Object.keys(valid) as (keyof typeof valid)[]) {
      for (const value of [0, -1, 1.5, Number.NaN, Number.MAX_SAFE_INTEGER + 1])
        assert.throws(
          () => new UnmatchedPreviewEventQueue({ ...valid, [key]: value }),
          RangeError,
          `${key}=${value}`,
        );
    }
  });

  test("counts valid events and reload markers in total usage", () => {
    const origin = daemonOrigin({} as JsonRpcConnection, "daemon", 1);
    const first = previewEvent("first", "full", 1);
    const second = previewEvent("second", "full", 1);
    const queue = new UnmatchedPreviewEventQueue({
      maxEventsPerPreview: 4,
      maxBytesPerPreview: 100_000,
      maxStreams: 4,
      maxTotalEvents: 4,
      maxTotalBytes: 100_000,
    });
    queue.enqueue(origin, first);
    queue.markReloadRequired(origin, "first");
    queue.enqueue(origin, second);
    assert.deepEqual(queue.usage, {
      streams: 2,
      totalEvents: 2,
      totalBytes:
        Buffer.byteLength(
          JSON.stringify({
            previewSessionId: "first",
            reloadRequired: true,
          }),
          "utf8",
        ) + previewEventUtf8Bytes(second),
      reloadMarkers: 1,
      failClosed: false,
    });

    queue.take(origin, "first");
    assert.deepEqual(queue.usage, {
      streams: 1,
      totalEvents: 1,
      totalBytes: previewEventUtf8Bytes(second),
      reloadMarkers: 0,
      failClosed: false,
    });
  });

  test("evicts all streams fail-closed when the global stream cap is hit", () => {
    const origin = daemonOrigin({} as JsonRpcConnection, "daemon", 1);
    const queue = globallyLimitedQueue({ maxStreams: 2 });
    queue.enqueue(origin, previewEvent("one", "full", 1));
    queue.markReloadRequired(origin, "two");
    queue.enqueue(origin, previewEvent("three", "full", 1));

    assert.deepEqual(queue.usage, {
      streams: 0,
      totalEvents: 0,
      totalBytes: 0,
      reloadMarkers: 0,
      failClosed: true,
    });
    for (const id of ["one", "two", "three", "future"])
      assert.deepEqual(queue.take(origin, id), {
        events: [],
        reloadRequired: true,
      });
  });

  test("fails closed when the global event or exact byte cap is hit", () => {
    const origin = daemonOrigin({} as JsonRpcConnection, "daemon", 1);
    const first = previewEvent("one", "full", 1);
    const second = previewEvent("two", "full", 1);
    const eventLimited = globallyLimitedQueue({ maxTotalEvents: 1 });
    eventLimited.enqueue(origin, first);
    eventLimited.enqueue(origin, second);
    assert.equal(eventLimited.usage.failClosed, true);

    const byteLimited = globallyLimitedQueue({
      maxTotalBytes:
        previewEventUtf8Bytes(first) + previewEventUtf8Bytes(second) - 1,
    });
    byteLimited.enqueue(origin, first);
    byteLimited.enqueue(origin, second);
    assert.equal(byteLimited.usage.failClosed, true);
  });

  test("keeps thousands of unique preview ids globally bounded", () => {
    const origin = daemonOrigin({} as JsonRpcConnection, "daemon", 1);
    const queue = globallyLimitedQueue({ maxStreams: 8 });
    for (let index = 0; index < 5000; index += 1)
      queue.enqueue(origin, previewEvent(`preview-${index}`, "full", 1));
    assert.deepEqual(queue.usage, {
      streams: 0,
      totalEvents: 0,
      totalBytes: 0,
      reloadMarkers: 0,
      failClosed: true,
    });
    assert.deepEqual(queue.take(origin, "preview-4999"), {
      events: [],
      reloadRequired: true,
    });
    const recovered = previewEvent("recovered", "full", 1);
    queue.clearAll();
    queue.enqueue(origin, recovered);
    assert.deepEqual(queue.take(origin, "recovered"), {
      events: [recovered],
      reloadRequired: false,
    });
  });

  test("clearOrigin releases exact global accounting without touching peers", () => {
    const firstOrigin = daemonOrigin(
      {} as JsonRpcConnection,
      "daemon-first",
      1,
    );
    const secondOrigin = daemonOrigin(
      {} as JsonRpcConnection,
      "daemon-second",
      1,
    );
    const first = previewEvent("first", "full", 1, "daemon-first");
    const second = previewEvent("second", "full", 1, "daemon-second");
    const queue = globallyLimitedQueue();
    queue.enqueue(firstOrigin, first);
    queue.markReloadRequired(firstOrigin, "marker");
    queue.enqueue(secondOrigin, second);
    queue.clearOrigin(firstOrigin);

    assert.deepEqual(queue.usage, {
      streams: 1,
      totalEvents: 1,
      totalBytes: previewEventUtf8Bytes(second),
      reloadMarkers: 0,
      failClosed: false,
    });
    assert.equal(queue.take(firstOrigin, "first"), undefined);
    assert.deepEqual(queue.take(secondOrigin, "second")?.events, [second]);
  });

  test("clearOrigin retires a queue-wide fail-closed marker", () => {
    const origin = daemonOrigin({} as JsonRpcConnection, "daemon", 1);
    const queue = globallyLimitedQueue({ maxStreams: 1 });
    queue.enqueue(origin, previewEvent("one", "full", 1));
    queue.enqueue(origin, previewEvent("two", "full", 1));
    assert.equal(queue.usage.failClosed, true);

    queue.clearOrigin(daemonOrigin({} as JsonRpcConnection, "other-daemon", 1));
    assert.equal(queue.usage.failClosed, true);
    queue.clearOrigin(origin);

    assert.deepEqual(queue.usage, {
      streams: 0,
      totalEvents: 0,
      totalBytes: 0,
      reloadMarkers: 0,
      failClosed: false,
    });
    const replacement = previewEvent("replacement", "full", 1);
    queue.enqueue(origin, replacement);
    assert.deepEqual(queue.take(origin, "replacement")?.events, [replacement]);
  });
}

function globallyLimitedQueue(
  overrides: Partial<{
    maxStreams: number;
    maxTotalEvents: number;
    maxTotalBytes: number;
  }> = {},
): UnmatchedPreviewEventQueue {
  return new UnmatchedPreviewEventQueue({
    maxEventsPerPreview: 64,
    maxBytesPerPreview: 100_000,
    maxStreams: 64,
    maxTotalEvents: 64,
    maxTotalBytes: 1_000_000,
    ...overrides,
  });
}

function previewCandidate(): {
  candidate: PreviewCandidate;
  runtime: WorkspaceRuntime;
} {
  const rpc = { closed: false } as JsonRpcConnection;
  const documentState = { sessionId: "document", version: 1 };
  const runtime = {
    removed: false,
    documents: new Map([["file:///document.md", documentState]]),
    previews: new Map(),
  } as unknown as WorkspaceRuntime;
  return {
    runtime,
    candidate: {
      origin: daemonOrigin(rpc, "daemon", 1),
      runtime,
      documentUri: "file:///document.md",
      documentState,
      documentSessionId: "document",
      documentVersion: 1,
      result: {
        previewSessionId: "preview",
        url: "http://preview",
        initialPublication: {
          type: "full",
          previewSessionId: "preview",
          documentVersion: 1,
          resultRenderRevision: 1,
          rendererFingerprint: "renderer",
          nodeIds: [],
          navigation: [],
          style: null,
          assets: [],
          html: "",
        },
      },
    } as PreviewCandidate,
  };
}

function previewDependencies(
  candidate: PreviewCandidate,
  runtime: WorkspaceRuntime,
  overrides: Partial<PreviewLifecycleDependencies> = {},
): PreviewLifecycleDependencies {
  const document = {
    languageId: "markdown",
    fileName: "document.md",
    version: 1,
    uri: { toString: () => "file:///document.md" },
  };
  return {
    activeEditor: () => ({ document, viewColumn: 1 }) as never,
    showNoDocument: () => assert.fail("document is present"),
    workspaceFor: () => ({}) as never,
    start: async () => runtime,
    sync: async () => undefined,
    request: async () => candidate,
    candidateCurrent: () => true,
    candidateIdentityCurrent: () => true,
    rejectCandidate: async () => undefined,
    handshake: async () => undefined,
    activate: async () => undefined,
    discardQueued: () => undefined,
    openExternal: async () => undefined,
    createPanel: () => {
      throw new Error("unused panel");
    },
    scriptUri: () => "script",
    random: () => "random",
    css: "",
    log: () => undefined,
    previewCurrent: () => true,
    markReload: () => undefined,
    reload: async () => true,
    report: () => undefined,
    dispose: () => undefined,
    notifyNavigation: () => undefined,
    ...overrides,
  };
}

function previewState(
  origin: DaemonOrigin,
  webview: { postMessage(message: unknown): Promise<boolean> },
): PreviewState {
  return {
    originRpc: origin.rpc,
    originDaemonInstanceId: origin.daemonInstanceId,
    originGeneration: origin.generation,
    documentUri: "file:///document.md",
    previewSessionId: "preview",
    target: "embeddedHtml",
    initialPublication: previewCandidate().candidate.result.initialPublication,
    renderRevision: 1,
    ready: false,
    messageToken: "token",
    panel: { webview } as never,
  };
}

function readinessDependencies(
  runtime: WorkspaceRuntime,
  queue: UnmatchedPreviewEventQueue,
  delivered: string[],
) {
  return {
    current: (owner: WorkspaceRuntime, preview: PreviewState) =>
      !owner.removed &&
      !preview.originRpc.closed &&
      owner.previews.get(preview.previewSessionId) === preview,
    take: (origin: DaemonOrigin, previewSessionId: string) =>
      queue.takeForDelivery(origin, previewSessionId),
    markReload: (origin: DaemonOrigin, previewSessionId: string) =>
      queue.markReloadRequired(origin, previewSessionId),
    deliver: async (
      _origin: DaemonOrigin,
      event: ReturnType<typeof previewEvent>,
    ) => {
      delivered.push(event.event.type);
      return true;
    },
    reload: async () => true,
    report: () => undefined,
  };
}

function actualReadinessDependencies(
  runtime: WorkspaceRuntime,
  queue: UnmatchedPreviewEventQueue,
  effects: {
    reload?: () => boolean | Promise<boolean>;
    navigate?: () => Promise<void>;
    report?: (error: unknown) => void;
  } = {},
) {
  const reload = effects.reload ?? (() => true);
  const navigate = effects.navigate ?? (async () => undefined);
  const report = effects.report ?? (() => undefined);
  return {
    current: (owner: WorkspaceRuntime, preview: PreviewState) =>
      !owner.removed &&
      !preview.originRpc.closed &&
      owner.previews.get(preview.previewSessionId) === preview,
    take: (origin: DaemonOrigin, previewSessionId: string) =>
      queue.takeForDelivery(origin, previewSessionId),
    markReload: (origin: DaemonOrigin, previewSessionId: string) =>
      queue.markReloadRequired(origin, previewSessionId),
    deliver: (
      origin: DaemonOrigin,
      event: Parameters<typeof handlePreviewEventLifecycle>[1],
    ) =>
      handlePreviewEventLifecycle(
        origin,
        event,
        [runtime],
        queue,
        {
          reload: () => void reload(),
          navigate: async () => navigate(),
          report,
        },
        true,
      ),
    reload: async () => (await reload()) !== false,
    report,
  };
}

function daemonOrigin(
  rpc: JsonRpcConnection,
  daemonInstanceId: string,
  generation: number,
): DaemonOrigin {
  return { rpc, daemonInstanceId, generation };
}

function previewEvent(
  previewSessionId: string,
  type: "full" | "patch" | "viewport",
  renderRevision: number,
  daemonInstanceId = "daemon",
) {
  const event =
    type === "viewport"
      ? { type, previewSessionId, renderRevision, nodeId: "node" }
      : type === "full"
        ? {
            type,
            previewSessionId,
            documentVersion: renderRevision,
            resultRenderRevision: renderRevision,
            rendererFingerprint: "renderer",
            nodeIds: [],
            navigation: [],
            style: null,
            assets: [],
            html: "",
          }
        : {
            type,
            previewSessionId,
            documentVersion: renderRevision,
            baseRenderRevision: renderRevision - 1,
            resultRenderRevision: renderRevision,
            baseRendererFingerprint: "renderer",
            resultRendererFingerprint: "renderer",
            navigation: [],
            style: null,
            operations: [],
          };
  return {
    daemonInstanceId,
    previewSessionId,
    renderRevision,
    event,
  } as const;
}

function sourceNavigationEvent(
  previewSessionId: string,
  daemonInstanceId = "daemon",
) {
  return {
    daemonInstanceId,
    previewSessionId,
    renderRevision: 1,
    event: {
      type: "selectSource",
      sourceRange: {
        byteStart: 0,
        byteEnd: 1,
        start: { line: 0, character: 0, encoding: "utf16" },
        end: { line: 0, character: 1, encoding: "utf16" },
      },
    },
  } as const;
}
