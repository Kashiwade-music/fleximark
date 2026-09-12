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
  test("rejects representative stale create-preview candidate dimensions", () => {
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
      { ...current, origin: { ...origin, generation: 8 } },
      { ...current, documentVersion: 4 },
    ];
    for (const stale of staleStates)
      assert.equal(previewCandidateIsCurrent(candidate, stale), false);
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

  test("builds a CSP-protected shell without embedding a publication", () => {
    const shell = embeddedPreviewShell(
      "nonce",
      "token",
      "vscode-webview:",
      "vscode-webview:/host.js",
      "body{}",
    );
    assert.match(shell, /Content-Security-Policy/);
    assert.match(shell, /default-src 'none'/);
    assert.match(shell, /script-src 'nonce-nonce' vscode-webview:/);
    assert.match(shell, /name="fleximark-message-token" content="token"/);
    assert.match(shell, /<main id="preview" class="markdown-body"><\/main>/);
    assert.match(
      shell,
      /<script nonce="nonce" src="vscode-webview:\/host\.js"><\/script>/,
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
    const pendingOpen = signal();
    const opened = signal();
    let activations = 0;
    let navigations = 0;
    const queue = new UnmatchedPreviewEventQueue();
    const opening = openPreviewLifecycle(
      "externalBrowser",
      previewDependencies(candidate, runtime, {
        openExternal: async () => {
          assert.equal(runtime.previews.get("preview")?.ready, false);
          opened.resolve();
          await pendingOpen.promise;
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
    await opened.promise;
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
    pendingOpen.resolve();
    await opening;
    assert.equal(activations, 0);
    assert.equal(navigations, 0);
  });

  test("queues external navigation until open succeeds and then drains only while current", async () => {
    const { candidate, runtime } = previewCandidate();
    const pendingOpen = signal();
    const opened = signal();
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
          opened.resolve();
          await pendingOpen.promise;
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
    await opened.promise;
    await handlePreviewEventLifecycle(
      candidate.origin,
      sourceNavigationEvent("preview"),
      [runtime],
      queue,
      { reload: assert.fail, navigate: async () => assert.fail() },
    );
    assert.equal(navigations, 0);
    pendingOpen.resolve();
    await opening;
    assert.equal(navigations, 1);
    assert.equal(runtime.previews.get("preview")?.ready, true);
  });

  test("holds a drifted initial external preview for a full event after reload rejection", async () => {
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
        reload: async () => {
          reloadAttempts += 1;
          throw reloadFailure;
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
    assert.equal(reloadAttempts, 1);
    assert.deepEqual(reloadCalls, []);
    assert.equal(activations, 0);
    assert.equal(disposals, 0);
    assert.equal(preview.ready, false);
    assert.equal(preview.reloadPending, true);
    assert.equal(queue.usage.reloadMarkers, 1);
    assert.deepEqual(reports, ["Error: initial external reload rejected"]);

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
    assert.equal(preview.ready, true);
    assert.equal(preview.reloadPending, false);
    assert.equal(queue.usage.streams, 0);
  });

  test("ignores an initial external rejection after same-id origin replacement", async () => {
    const { candidate, runtime } = previewCandidate();
    const opened = signal();
    const pendingOpen = signal();
    const discarded: string[] = [];
    const opening = openPreviewLifecycle(
      "externalBrowser",
      previewDependencies(candidate, runtime, {
        openExternal: async () => {
          opened.resolve();
          await pendingOpen.promise;
        },
        discardQueued: (_origin, previewSessionId) => {
          discarded.push(previewSessionId);
        },
      }),
    );
    await opened.promise;
    const preview = runtime.previews.get("preview");
    assert.ok(preview);
    const replacementRpc = { closed: false } as JsonRpcConnection;
    preview.originRpc = replacementRpc;
    preview.originDaemonInstanceId = "replacement-daemon";
    preview.originGeneration = 2;
    preview.ready = false;

    pendingOpen.reject(new Error("browser failed"));
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
    let handshake = signal();
    let disposals = 0;
    const reports: unknown[] = [];
    await openPreviewLifecycle(
      "embeddedHtml",
      previewDependencies(candidate, runtime, {
        createPanel: () => panel as never,
        handshake: () => handshake.promise,
        previewCurrent: () => true,
        report: (error) => reports.push(error),
        dispose: () => {
          disposals += 1;
        },
      }),
    );

    const currentFailure = new Error("current ready failed");
    messageListener({ type: "ready" });
    handshake.reject(currentFailure);
    await Promise.resolve();
    await Promise.resolve();
    assert.deepEqual(reports, [currentFailure]);

    handshake = signal();
    const retiredFailure = new Error("retired ready failed");
    messageListener({ type: "ready" });
    const recreated = runtime.previews.get("preview");
    assert.ok(recreated);
    recreated.originRpc = {} as JsonRpcConnection;
    recreated.originGeneration += 1;
    recreated.originDaemonInstanceId = "replacement-daemon";
    handshake.reject(retiredFailure);
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
    const post = deferred<boolean>();
    let posts = 0;
    const rpc = { closed: false } as JsonRpcConnection;
    const origin = daemonOrigin(rpc, "daemon", 2);
    const queue = new UnmatchedPreviewEventQueue();
    const preview = previewState(origin, {
      postMessage: () => {
        posts += 1;
        return post.promise;
      },
    });
    const runtime = runtimeWithPreview(preview);
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
    post.resolve(true);
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
      const firstPost = deferred<boolean>();
      let initializePosts = 0;
      const origin = daemonOrigin(
        { closed: false } as JsonRpcConnection,
        "daemon",
        1,
      );
      const preview = previewState(origin, {
        postMessage: () => {
          initializePosts += 1;
          return initializePosts === 1
            ? firstPost.promise
            : Promise.resolve(true);
        },
      });
      const runtime = runtimeWithPreview(preview);
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
      if (failure === "reject") firstPost.reject(error);
      else firstPost.resolve(false);
      await first;

      assert.equal(initializePosts, 2);
      assert.equal(reloads, 2);
      assert.equal(preview.ready, false);
      assert.equal(preview.reloadPending, true);
      assert.equal(queue.usage.reloadMarkers, 1);
      assert.deepEqual(reports, failure === "reject" ? [error] : []);
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
    const runtime = runtimeWithPreview(preview);
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
      const post = deferred<boolean>();
      const rpc = { closed: false } as JsonRpcConnection;
      const origin = daemonOrigin(rpc, "daemon", 1);
      const queue = new UnmatchedPreviewEventQueue();
      const preview = previewState(origin, {
        postMessage: () => post.promise,
      });
      const runtime = runtimeWithPreview(preview);
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
        post.resolve(true);
      } else if (outcome === "closed") {
        Object.assign(rpc, { closed: true });
        post.resolve(true);
      } else if (outcome === "reject") post.reject(new Error("closed"));
      else post.resolve(false);
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
    const runtime = runtimeWithPreview(preview);
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
    const runtime = runtimeWithPreview(preview);
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
    const previous = previousPreview(candidate);
    runtime.previews.set("old-preview", previous);
    const rejected: string[] = [];
    await recreatePreviews(runtime, candidate, {
      request: async () => {
        runtime.previews.delete("old-preview");
        return candidate;
      },
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

  test("retains and reloads a recreate when initial delivery rejects", async () => {
    const { candidate, runtime } = previewCandidate();
    const previous = previousPreview(candidate, {
      target: "embeddedHtml",
      originDaemonInstanceId: "old-daemon",
      originGeneration: 0,
      panel: {
        webview: {
          postMessage: () => Promise.reject(new Error("post failed")),
        },
      },
    } as unknown as Partial<PreviewState>);
    runtime.previews.set("old-preview", previous);
    let reloaded = 0;
    let disposed = 0;
    const reports: string[] = [];
    const queue = new UnmatchedPreviewEventQueue();
    await recreatePreviews(runtime, candidate, {
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
    assert.deepEqual(reports, ["Error: post failed"]);
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

    await recreatePreviews(runtime, candidate, {
      candidateCurrent: () => versionCurrent,
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

    await recreatePreviews(runtime, candidate, {
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

    await recreatePreviews(runtime, candidate, {
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
    const handshakeStarted = signal();
    const pendingHandshake = signal();
    let reloads = 0;
    let disposals = 0;
    const reports: unknown[] = [];

    const recreating = recreatePreviews(runtime, candidate, {
      reject: async () => assert.fail("committed candidate is retained"),
      dispose: async () => {
        disposals += 1;
      },
      handshake: async () => {
        handshakeStarted.resolve();
        await pendingHandshake.promise;
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
    await handshakeStarted.promise;
    const replacementRpc = { closed: false } as JsonRpcConnection;
    runtime.previews.delete("preview");
    previous.previewSessionId = "replacement-preview";
    previous.originRpc = replacementRpc;
    previous.originDaemonInstanceId = "replacement-daemon";
    previous.originGeneration = 2;
    runtime.previews.set("replacement-preview", previous);

    pendingHandshake.reject(new Error("old handshake failed"));
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
    const activationStarted = signal();
    const pendingActivation = signal();
    let disposals = 0;
    const reports: unknown[] = [];

    const recreating = recreatePreviews(runtime, candidate, {
      reject: async () => assert.fail("committed candidate is retained"),
      dispose: async () => {
        disposals += 1;
      },
      handshake: async (_owner, item) => {
        item.ready = true;
      },
      activate: async () => {
        activationStarted.resolve();
        await pendingActivation.promise;
      },
      discardQueued: () => undefined,
      markReload: () => undefined,
      reload: async () => true,
      openExternal: async () => assert.fail("embedded preview"),
      report: (error) => reports.push(error),
    });
    await activationStarted.promise;
    const replacementRpc = { closed: false } as JsonRpcConnection;
    previous.originRpc = replacementRpc;
    previous.originDaemonInstanceId = "replacement-daemon";
    previous.originGeneration = 2;
    previous.ready = false;

    pendingActivation.reject(new Error("old activation failed"));
    await recreating;

    assert.equal(disposals, 0);
    assert.deepEqual(reports, []);
    assert.equal(runtime.previews.get("preview"), previous);
    assert.equal(previous.originRpc, replacementRpc);
    assert.equal(previous.ready, false);
  });

  test("reports a current external recreation rejection and restores readiness", async () => {
    const { candidate, runtime } = previewCandidate();
    const previous = previousPreview(candidate);
    runtime.previews.set("old-preview", previous);
    const failure = new Error("current browser open failed");
    const reports: unknown[] = [];

    await recreatePreviews(runtime, candidate, {
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
    const oldPost = deferred<boolean>();
    const posts: string[] = [];
    const previous = previewState(oldOrigin, {
      postMessage: (message: { publication: { previewSessionId: string } }) => {
        posts.push(message.publication.previewSessionId);
        return posts.length === 1 ? oldPost.promise : Promise.resolve(true);
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

    await recreatePreviews(runtime, candidate, {
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

    oldPost.resolve(false);
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
      const previous = previousPreview(candidate, {
        originRpc: oldOrigin.rpc,
        originDaemonInstanceId: oldOrigin.daemonInstanceId,
        originGeneration: oldOrigin.generation,
      });
      runtime.previews.set("old-preview", previous);
      let disposals = 0;
      let rejections = 0;
      const reports: unknown[] = [];
      const requestFailure = new Error("recreate request failed");
      const missingUrl = {
        ...candidate,
        result: { ...candidate.result, url: undefined },
      };
      await recreatePreviews(runtime, candidate, {
        request: async () => {
          if (failure === "throw") throw requestFailure;
          if (failure === "missing-url") return missingUrl;
          if (failure === "stale-candidate") return candidate;
          return undefined;
        },
        candidateCurrent: () => failure !== "stale-candidate",
        reject: async () => {
          rejections += 1;
        },
        dispose: async (owner, item) => {
          disposals += 1;
          owner.previews.delete(item.previewSessionId);
        },
        discardQueued: () => undefined,
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

  test("keeps an old preview for replay when recovery throws without a replacement origin", async () => {
    const { candidate, runtime } = previewCandidate();
    const previous = previousPreview(candidate);
    runtime.previews.set("old-preview", previous);
    let disposals = 0;
    const reports: unknown[] = [];
    const failure = new Error("origin closed during recovery");

    await recreatePreviews(runtime, candidate, {
      request: async () => {
        throw failure;
      },
      currentOrigin: () => undefined,
      candidateCurrent: () => assert.fail("there is no candidate"),
      candidateIdentityCurrent: () => assert.fail("there is no candidate"),
      dispose: async () => {
        disposals += 1;
      },
      report: (error) => reports.push(error),
    });

    assert.equal(disposals, 0);
    assert.equal(runtime.previews.get("old-preview"), previous);
    assert.deepEqual(reports, [failure]);
  });

  test("does not activate a recreated external preview removed while URL opening is pending", async () => {
    const { candidate, runtime } = previewCandidate();
    const previous = previousPreview(candidate, {
      originDaemonInstanceId: "old-daemon",
      originGeneration: 0,
    });
    runtime.previews.set("old-preview", previous);
    const pendingOpen = signal();
    const opened = signal();
    let activations = 0;
    const recreating = recreatePreviews(runtime, candidate, {
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
        opened.resolve();
        await pendingOpen.promise;
      },
      report: assert.fail,
    });
    await opened.promise;
    runtime.removed = true;
    runtime.previews.clear();
    pendingOpen.resolve();
    await recreating;
    assert.equal(activations, 0);
  });

  test("reloads a drifted recreated external preview and waits for an authoritative full event", async () => {
    const { candidate, runtime } = previewCandidate();
    const previous = previousPreview(candidate);
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

    await recreatePreviews(runtime, candidate, {
      candidateCurrent: () => versionCurrent,
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
    const pending = signal();
    const order: string[] = [];
    const rpc = {
      request: () => {
        order.push("rpc");
        return pending.promise;
      },
    } as unknown as JsonRpcConnection;
    const preview = {
      originRpc: rpc,
      originDaemonInstanceId: "daemon",
      originGeneration: 1,
      previewSessionId: "preview",
      panel: { dispose: () => order.push("panel") },
    } as never;
    const runtime = runtimeWithPreview(preview);
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
    pending.resolve();
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
    const runtime = runtimeWithPreview(preview);
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
    const runtime = runtimeWithPreview(preview);
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

  test("suppresses a late reload error after same-id origin replacement", async () => {
    const pendingRequest = signal();
    const rpc = {
      closed: false,
      request: () => pendingRequest.promise,
    } as unknown as JsonRpcConnection;
    const origin = daemonOrigin(rpc, "daemon", 4);
    const preview = previewState(origin, {
      postMessage: async () => true,
    });
    const runtime = runtimeWithPreview(preview);
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
    pendingRequest.reject(new Error("stale"));

    assert.equal(await reloading, false);
    assert.deepEqual(reports, []);
    assert.equal(runtime.previews.get("preview"), preview);
    assert.equal(preview.originRpc, replacementRpc);
  });

  test("suppresses navigation effects for representative stale preview identities", async () => {
    for (const stale of ["membership", "generation", "editor"] as const) {
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
      const runtime = runtimeWithPreview(preview);
      const shown = deferred<unknown>();
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
          showEditor: () => shown.promise as never,
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
      if (stale === "generation") preview.originGeneration = 2;
      if (stale === "editor") editorDocument = {};
      shown.resolve({ document: editorDocument });
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

  test("separates reused preview ids by every origin identity dimension", () => {
    const sharedRpc = {} as JsonRpcConnection;
    const cases = [
      [
        daemonOrigin({} as JsonRpcConnection, "daemon", 1),
        daemonOrigin({} as JsonRpcConnection, "daemon", 1),
        "daemon",
      ],
      [
        daemonOrigin(sharedRpc, "daemon-first", 1),
        daemonOrigin(sharedRpc, "daemon-second", 1),
        "daemon-second",
      ],
      [
        daemonOrigin(sharedRpc, "daemon", 3),
        daemonOrigin(sharedRpc, "daemon", 4),
        "daemon",
      ],
    ] as const;
    for (const [firstOrigin, secondOrigin, secondDaemon] of cases) {
      const queue = new UnmatchedPreviewEventQueue();
      const first = previewEvent(
        "shared",
        "full",
        1,
        firstOrigin.daemonInstanceId,
      );
      const second = previewEvent("shared", "full", 9, secondDaemon);
      queue.enqueue(firstOrigin, first);
      queue.enqueue(secondOrigin, second);

      assert.deepEqual(queue.take(secondOrigin, "shared")?.events, [second]);
      assert.deepEqual(queue.take(firstOrigin, "shared")?.events, [first]);
    }
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

  test("measures the exact UTF-8 JSON bytes", () => {
    const event = previewEvent("日本語-😀", "full", 1);
    if (event.event.type !== "full") assert.fail("full event expected");
    event.event.html = "é-日本語-😀-\u0000-\n-\t";
    assert.equal(
      previewEventUtf8Bytes(event),
      Buffer.byteLength(JSON.stringify(event), "utf8"),
    );
  });

  test("rejects representative invalid local and global queue limits", () => {
    const valid = {
      maxEventsPerPreview: 2,
      maxBytesPerPreview: 2,
      maxStreams: 2,
      maxTotalEvents: 2,
      maxTotalBytes: 2,
    };
    const invalid = [
      ["maxEventsPerPreview", 0],
      ["maxBytesPerPreview", 1.5],
      ["maxStreams", Number.NaN],
      ["maxTotalBytes", Number.MAX_SAFE_INTEGER + 1],
    ] as const;
    for (const [key, value] of invalid)
      assert.throws(
        () => new UnmatchedPreviewEventQueue({ ...valid, [key]: value }),
        RangeError,
        `${key}=${value}`,
      );
  });

  test("a reload marker replaces queued events for delivery", () => {
    const origin = daemonOrigin({} as JsonRpcConnection, "daemon", 1);
    const first = previewEvent("first", "full", 1);
    const queue = new UnmatchedPreviewEventQueue();
    queue.enqueue(origin, first);
    queue.markReloadRequired(origin, "first");
    assert.deepEqual(queue.take(origin, "first"), {
      events: [],
      reloadRequired: true,
    });
    assert.equal(queue.take(origin, "first"), undefined);
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

function deferred<T>(): {
  promise: Promise<T>;
  resolve(value: T): void;
  reject(reason?: unknown): void;
} {
  let resolve!: (value: T) => void;
  let reject!: (reason?: unknown) => void;
  const promise = new Promise<T>((accept, decline) => {
    resolve = accept;
    reject = decline;
  });
  return { promise, resolve, reject };
}

function signal(): {
  promise: Promise<void>;
  resolve(): void;
  reject(reason?: unknown): void;
} {
  const value = deferred<undefined>();
  return {
    promise: value.promise,
    resolve: () => value.resolve(undefined),
    reject: value.reject,
  };
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

function previousPreview(
  candidate: PreviewCandidate,
  overrides: Partial<PreviewState> = {},
): PreviewState {
  return {
    documentUri: "file:///document.md",
    previewSessionId: "old-preview",
    target: "externalBrowser",
    originRpc: candidate.origin.rpc,
    originDaemonInstanceId: candidate.origin.daemonInstanceId,
    originGeneration: candidate.origin.generation,
    initialPublication: candidate.result.initialPublication,
    renderRevision: 1,
    ...overrides,
  } as PreviewState;
}

function recreatePreviews(
  runtime: WorkspaceRuntime,
  candidate: PreviewCandidate,
  overrides: Partial<Parameters<typeof recreatePreviewsLifecycle>[1]>,
): Promise<void> {
  return recreatePreviewsLifecycle(runtime, {
    document: () => ({}) as never,
    request: async () => candidate,
    currentOrigin: () => candidate.origin,
    candidateCurrent: () => true,
    candidateIdentityCurrent: () => true,
    reject: async () => assert.fail("unexpected candidate rejection"),
    dispose: async () => assert.fail("unexpected preview disposal"),
    handshake: async () => assert.fail("unexpected embedded handshake"),
    activate: async () => assert.fail("unexpected preview activation"),
    discardQueued: () => assert.fail("unexpected queue discard"),
    markReload: () => assert.fail("unexpected reload marker"),
    reload: async () => assert.fail("unexpected preview reload"),
    openExternal: async () => assert.fail("unexpected external open"),
    report: assert.fail,
    ...overrides,
  });
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

function runtimeWithPreview(preview: PreviewState): WorkspaceRuntime {
  return {
    removed: false,
    previews: new Map([[preview.previewSessionId, preview]]),
  } as unknown as WorkspaceRuntime;
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
