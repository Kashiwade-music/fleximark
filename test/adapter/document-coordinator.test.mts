import * as assert from "node:assert/strict";
import { createHash } from "node:crypto";

import {
  type DaemonOrigin,
  type DocumentClock,
  changeDocument,
  checkpointDocument,
  syncDocument,
} from "../../adapters/vscode/src/document-coordinator.mjs";
import type { JsonRpcConnection } from "../../adapters/vscode/src/rpc.mjs";
import type {
  DocumentState,
  WorkspaceRuntime,
} from "../../adapters/vscode/src/runtime-state.mjs";

export const suiteName = "Document coordinator";

export function suite(): void {
  test("an old attach cannot commit into a closed and reopened document state", async () => {
    let resolveAttach!: (value: { documentSessionId: string }) => void;
    const attach = new Promise<{ documentSessionId: string }>((resolve) => {
      resolveAttach = resolve;
    });
    const rpc = {
      closed: false,
      notifyLsp: () => undefined,
      request: () => attach,
    } as unknown as JsonRpcConnection;
    const origin: DaemonOrigin = {
      rpc,
      daemonInstanceId: "daemon",
      generation: 1,
    };
    const uri = "file:///document.md";
    const document = textDocument(uri, 1, "first");
    const runtime = {
      removed: false,
      documents: new Map(),
      previews: new Map(),
    } as unknown as WorkspaceRuntime;
    const syncing = syncDocument(
      runtime,
      document,
      origin,
      (owner, opened, state) =>
        owner.documents.get(opened.uri.toString()) === state,
      () => assert.fail("the stale attach must not emit a catch-up change"),
    );
    const oldState = runtime.documents.get(uri);
    assert.ok(oldState?.syncing);
    runtime.documents.delete(uri);
    const reopened: DocumentState = {
      sessionId: "new-session",
      version: 2,
      syncing: Promise.resolve(),
    };
    runtime.documents.set(uri, reopened);

    resolveAttach({ documentSessionId: "old-session" });
    await syncing;
    assert.equal(runtime.documents.get(uri), reopened);
    assert.equal(reopened.sessionId, "new-session");
    assert.notEqual(reopened.syncing, undefined);
    assert.equal(oldState?.sessionId, undefined);
  });

  test("resets the exact 150ms checkpoint and fires with latest text, version, and hash", async () => {
    const scheduled: {
      callback: () => void;
      delay: number;
      handle: NodeJS.Timeout;
    }[] = [];
    const cancelled: NodeJS.Timeout[] = [];
    const clock: DocumentClock = {
      schedule(callback, delay) {
        const handle = { id: scheduled.length } as unknown as NodeJS.Timeout;
        scheduled.push({ callback, delay, handle });
        return handle;
      },
      cancel(handle) {
        cancelled.push(handle);
      },
    };
    const requests: { method: string; params: unknown }[] = [];
    const rpc = {
      closed: false,
      notifyLsp: () => undefined,
      request(method: string, params: unknown) {
        requests.push({ method, params });
        return Promise.resolve(null);
      },
    } as unknown as JsonRpcConnection;
    const origin: DaemonOrigin = {
      rpc,
      daemonInstanceId: "daemon",
      generation: 1,
    };
    const uri = "file:///document.md";
    const state: DocumentState = { sessionId: "session", version: 1 };
    const runtime = {
      documents: new Map([[uri, state]]),
    } as unknown as WorkspaceRuntime;
    const first = textDocument(uri, 2, "first edit");
    const latest = textDocument(uri, 3, "latest edit");
    const fail = (error: unknown) => assert.fail(String(error));
    changeDocument(runtime, first, rpc, async () => undefined, fail, clock);
    changeDocument(
      runtime,
      latest,
      rpc,
      () => checkpointDocument(latest, state, origin),
      fail,
      clock,
    );
    assert.deepEqual(
      scheduled.map(({ delay }) => delay),
      [150, 150],
    );
    assert.deepEqual(cancelled, [scheduled[0].handle]);

    scheduled[1].callback();
    await Promise.resolve();
    assert.deepEqual(requests, [
      {
        method: "fleximark/checkpointDocument",
        params: {
          daemonInstanceId: "daemon",
          documentSessionId: "session",
          documentVersion: 3,
          contentHash: createHash("sha256").update("latest edit").digest("hex"),
        },
      },
    ]);
  });
}

function textDocument(uri: string, version: number, text: string) {
  return {
    languageId: "markdown",
    uri: { toString: () => uri },
    version,
    getText: () => text,
  } as never;
}
