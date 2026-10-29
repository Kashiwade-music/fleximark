import * as vscode from "vscode";

/** Requires both the folder event and its settled workspace membership. */
function changeFolders(
  uris: readonly vscode.Uri[],
  kind: "added" | "removed",
  apply: () => boolean,
  label: string,
): Promise<void> {
  const expected = uris.map((uri) => uri.toString());
  return new Promise<void>((resolve, reject) => {
    const observed = new Set<string>();
    let settled = false;
    const finish = (error?: unknown) => {
      if (settled) return;
      settled = true;
      clearInterval(interval);
      clearTimeout(timeout);
      subscription.dispose();
      if (error) reject(error);
      else resolve();
    };
    const check = () => {
      const current = new Set(
        (vscode.workspace.workspaceFolders ?? []).map(({ uri }) =>
          uri.toString(),
        ),
      );
      if (
        expected.every(
          (uri) => observed.has(uri) && current.has(uri) === (kind === "added"),
        )
      )
        finish();
    };
    const subscription = vscode.workspace.onDidChangeWorkspaceFolders(
      (event) => {
        for (const { uri } of event[kind]) observed.add(uri.toString());
        check();
      },
    );
    const interval = setInterval(check, 25);
    const timeout = setTimeout(
      () => finish(new Error(`Timed out ${label}`)),
      5_000,
    );
    try {
      if (!apply()) finish(new Error(`Could not ${label}`));
      else check();
    } catch (error) {
      finish(error);
    }
  });
}

export function addWorkspaceFolders(
  folders: readonly { uri: vscode.Uri; name: string }[],
): Promise<void> {
  return changeFolders(
    folders.map(({ uri }) => uri),
    "added",
    () =>
      vscode.workspace.updateWorkspaceFolders(
        vscode.workspace.workspaceFolders?.length ?? 0,
        0,
        ...folders,
      ),
    "add FlexiMark workspace folders",
  );
}

export async function removeWorkspaceFolder(uri: vscode.Uri): Promise<void> {
  const index = vscode.workspace.workspaceFolders?.findIndex(
    (folder) => folder.uri.toString() === uri.toString(),
  );
  if (index === undefined || index < 0) return;
  await changeFolders(
    [uri],
    "removed",
    () => vscode.workspace.updateWorkspaceFolders(index, 1),
    `remove workspace folder ${uri.toString()}`,
  );
}
