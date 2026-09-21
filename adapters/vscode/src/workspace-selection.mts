import type { ViewColumn } from "vscode";

interface VisibleSourceEditor {
  readonly document: {
    readonly uri: {
      toString(): string;
    };
  };
  readonly viewColumn?: ViewColumn;
}

export function findVisibleSourceEditor<T extends VisibleSourceEditor>(
  editors: readonly T[],
  documentUri: string,
  sourceViewColumn?: ViewColumn,
): T | undefined {
  const matchingEditors = editors.filter(
    (editor) => editor.document.uri.toString() === documentUri,
  );
  return (
    matchingEditors.find((editor) => editor.viewColumn === sourceViewColumn) ??
    matchingEditors[0]
  );
}

export async function selectWorkspaceUri(
  activeWorkspaceUri: string | undefined,
  workspaces: readonly { label: string; uri: string }[],
  pick: (
    items: readonly { label: string; uri: string }[],
    options: { placeHolder: string },
  ) => PromiseLike<{ label: string; uri: string } | undefined>,
): Promise<string | undefined> {
  return selectWorkspaceUriWithPlaceholder(
    activeWorkspaceUri,
    workspaces,
    pick,
    "Select a FlexiMark workspace",
  );
}

export async function selectWorkspaceUriWithPlaceholder(
  activeWorkspaceUri: string | undefined,
  workspaces: readonly { label: string; uri: string }[],
  pick: (
    items: readonly { label: string; uri: string }[],
    options: { placeHolder: string },
  ) => PromiseLike<{ label: string; uri: string } | undefined>,
  placeHolder: string,
): Promise<string | undefined> {
  if (
    activeWorkspaceUri &&
    workspaces.some((workspace) => workspace.uri === activeWorkspaceUri)
  )
    return activeWorkspaceUri;
  if (workspaces.length === 1) return workspaces[0].uri;
  return (await pick(workspaces, { placeHolder }))?.uri;
}
