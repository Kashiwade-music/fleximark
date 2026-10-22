export interface Category {
  name: string;
  children: Category[];
}
export interface WorkspaceState {
  command?: string;
  context?: { workspaceUri?: string; documentUri?: string };
  workspaces: { uri: string; name: string }[];
  documents: {
    uri: string;
    version?: number;
    workspaceUri?: string;
    title?: string;
  }[];
  noteOptions?: { categories: Category[]; templates: string[] };
}
export interface CommandResult {
  message?: string;
  openUri?: string;
  data?: unknown;
}
export type WorkspaceCommand =
  | "createNote"
  | "initializeWorkspace"
  | "editTheme"
  | "collectAdmonitions"
  | "exportHtml";
export interface CommandInput {
  command: WorkspaceCommand;
  workspaceUri: string;
  documentUri?: string;
  documentVersion?: number;
  noteCategoryPath?: string[];
  noteTemplate?: string;
  noteFileName?: string;
  destinationUri?: string;
}
const object = (v: unknown): v is Record<string, unknown> =>
  v !== null && typeof v === "object" && !Array.isArray(v);
const text = (v: unknown): v is string => typeof v === "string";
const category = (v: unknown): v is Category =>
  object(v) &&
  text(v.name) &&
  Array.isArray(v.children) &&
  v.children.every(category);
export function isWorkspaceState(v: unknown): v is WorkspaceState {
  return (
    object(v) &&
    Array.isArray(v.workspaces) &&
    v.workspaces.every((w) => object(w) && text(w.uri) && text(w.name)) &&
    Array.isArray(v.documents) &&
    v.documents.every(
      (d) =>
        object(d) &&
        text(d.uri) &&
        (d.version === undefined || Number.isSafeInteger(d.version)) &&
        (d.workspaceUri === undefined || text(d.workspaceUri)) &&
        (d.title === undefined || text(d.title)),
    ) &&
    (v.command === undefined || text(v.command)) &&
    (v.context === undefined ||
      (object(v.context) &&
        (v.context.workspaceUri === undefined ||
          text(v.context.workspaceUri)) &&
        (v.context.documentUri === undefined ||
          text(v.context.documentUri)))) &&
    (v.noteOptions === undefined ||
      (object(v.noteOptions) &&
        Array.isArray(v.noteOptions.categories) &&
        v.noteOptions.categories.every(category) &&
        Array.isArray(v.noteOptions.templates) &&
        v.noteOptions.templates.every(text)))
  );
}
export function isCommandResult(v: unknown): v is CommandResult {
  return (
    object(v) &&
    (v.message === undefined || text(v.message)) &&
    (v.openUri === undefined || text(v.openUri))
  );
}
export function categoryPaths(
  categories: Category[],
  parents: string[] = [],
): string[][] {
  return categories.flatMap((c) => {
    const path = [...parents, c.name];
    return [path, ...categoryPaths(c.children, path)];
  });
}
export function validateFileName(name: string): boolean {
  return (
    !!name.trim() &&
    name.length <= 255 &&
    !/[\\/]/.test(name) &&
    Array.from(name).every((character) => character.charCodeAt(0) >= 32) &&
    name !== "." &&
    name !== ".."
  );
}
export async function readResponse<T>(
  response: Response,
  validate: (value: unknown) => value is T,
): Promise<T> {
  const value: unknown = await response.json();
  if (!response.ok)
    throw new Error(
      object(value) && text(value.message)
        ? value.message
        : "Request failed (" + response.status + ").",
    );
  if (!validate(value))
    throw new Error("The server returned an invalid response.");
  return value;
}
