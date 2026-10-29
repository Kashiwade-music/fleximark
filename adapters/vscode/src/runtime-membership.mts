import type * as vscode from "vscode";

import type { DocumentRuntime } from "./runtime-state.mjs";

export const DETACHED_RUNTIME_KEY = "fleximark:detached";
export function runtimeKey(workspace?: vscode.WorkspaceFolder): string {
  return workspace?.uri.toString() ?? DETACHED_RUNTIME_KEY;
}
export function ensureRuntime(
  runtimes: Map<string, DocumentRuntime>,
  workspace?: vscode.WorkspaceFolder,
): DocumentRuntime {
  const key = runtimeKey(workspace);
  let runtime = runtimes.get(key);
  if (!runtime) {
    runtime = {
      workspace,
      documents: new Map(),
      previews: new Map(),
      removed: false,
    };
    runtimes.set(key, runtime);
  }
  return runtime;
}
export function reconcileRuntimeMembership(
  runtimes: Map<string, DocumentRuntime>,
  workspaceFor: (uri: string) => vscode.WorkspaceFolder | undefined,
  removedWorkspace?: string,
): void {
  const target = (uri: string) => {
    const workspace = workspaceFor(uri);
    return ensureRuntime(
      runtimes,
      workspace?.uri.toString() === removedWorkspace ? undefined : workspace,
    );
  };
  for (const runtime of [...runtimes.values()]) {
    for (const [uri, document] of [...runtime.documents]) {
      const owner = target(uri);
      if (owner === runtime) continue;
      runtime.documents.delete(uri);
      owner.documents.set(uri, document);
    }
    for (const [id, preview] of [...runtime.previews]) {
      const owner = target(preview.documentUri);
      if (owner === runtime) continue;
      runtime.previews.delete(id);
      owner.previews.set(id, preview);
    }
  }
  for (const [key, runtime] of [...runtimes]) {
    if (
      key === removedWorkspace &&
      runtime.documents.size === 0 &&
      runtime.previews.size === 0
    ) {
      runtime.removed = true;
      runtimes.delete(key);
    }
  }
}
