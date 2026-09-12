export interface LegacyCategoryTree {
  readonly [name: string]: LegacyCategoryTree;
}

export interface LegacyWorkspaceSettings {
  readonly defaultPreviewMode?: unknown;
  readonly noteCategories?: unknown;
  readonly noteFileNamePrefix?: unknown;
  readonly noteFileNameSuffix?: unknown;
  readonly noteTemplates?: unknown;
}

export interface LegacyWorkspaceState {
  readonly hasLegacyTheme: boolean;
  readonly hasLegacyPlugin: boolean;
  readonly hasAttachments: boolean;
  readonly settings: LegacyWorkspaceSettings;
}

export interface WorkspaceMigrationResult {
  readonly legacyPluginRetained: boolean;
}

export interface WorkspaceMigrationDependencies<Workspace> {
  key(workspace: Workspace): string;
  detect(workspace: Workspace): Promise<LegacyWorkspaceState | undefined>;
  confirm(workspace: Workspace, state: LegacyWorkspaceState): Promise<boolean>;
  migrate(
    workspace: Workspace,
    state: LegacyWorkspaceState,
  ): Promise<WorkspaceMigrationResult>;
  completed(
    workspace: Workspace,
    result: WorkspaceMigrationResult,
  ): Promise<void>;
}

/**
 * Offers migration at most once per open workspace. Nothing is persisted, so a
 * cancelled migration is offered again after the workspace is reopened.
 */
export class WorkspaceMigrationController<Workspace> {
  readonly #handled = new Set<string>();
  readonly #dependencies: WorkspaceMigrationDependencies<Workspace>;

  constructor(dependencies: WorkspaceMigrationDependencies<Workspace>) {
    this.#dependencies = dependencies;
  }

  async offer(workspace: Workspace): Promise<void> {
    const key = this.#dependencies.key(workspace);
    if (this.#handled.has(key)) return;
    this.#handled.add(key);
    const state = await this.#dependencies.detect(workspace);
    if (!state || !(await this.#dependencies.confirm(workspace, state))) return;
    const result = await this.#dependencies.migrate(workspace, state);
    await this.#dependencies.completed(workspace, result);
  }

  forget(workspace: Workspace): void {
    this.#handled.delete(this.#dependencies.key(workspace));
  }
}

export function createMigratedConfig(
  settings: LegacyWorkspaceSettings,
  hasAttachments: boolean,
): string {
  const prefix =
    typeof settings.noteFileNamePrefix === "string"
      ? settings.noteFileNamePrefix
      : "";
  const suffix =
    typeof settings.noteFileNameSuffix === "string"
      ? settings.noteFileNameSuffix
      : "";
  const categories = flattenLegacyCategories(settings.noteCategories);
  const templates = validTemplates(settings.noteTemplates);
  const lines = [
    "# FlexiMark workspace configuration",
    "# Migrated from the legacy VS Code workspace format.",
    "schema_version = 1",
    "",
    "[notes]",
    `file_name_prefix = ${tomlString(prefix)}`,
    `file_name_suffix = ${tomlString(suffix)}`,
  ];

  if (categories.length) {
    lines.push("", "[notes.categories]");
    for (const [label, value] of categories)
      lines.push(`${tomlString(label)} = ${tomlString(value)}`);
  }
  if (templates.length) {
    lines.push("", "[notes.templates]");
    for (const [name, value] of templates)
      lines.push(`${tomlString(name)} = [${value.map(tomlString).join(", ")}]`);
  }
  if (hasAttachments)
    lines.push("", "[assets]", `roots = [${tomlString("attachments")}]`);
  lines.push(
    "",
    "[security]",
    'raw_html_preview = "escape"',
    'raw_html_export = "reject"',
    "",
  );
  return lines.join("\n");
}

export function flattenLegacyCategories(
  value: unknown,
): readonly (readonly [label: string, path: string])[] {
  if (!isRecord(value)) return [];
  const result: [string, string][] = [];
  const pending = Object.entries(value)
    .reverse()
    .map(([name, child]) => ({ child, parts: [name] }));
  while (pending.length) {
    const next = pending.pop();
    if (!next || !safeCategoryPart(next.parts.at(-1) ?? "")) continue;
    result.push([next.parts.join(" / "), next.parts.join("/")]);
    if (!isRecord(next.child)) continue;
    for (const [name, child] of Object.entries(next.child).reverse())
      pending.push({ child, parts: [...next.parts, name] });
  }
  return result;
}

function validTemplates(
  value: unknown,
): readonly (readonly [name: string, lines: readonly string[]])[] {
  if (!isRecord(value)) return [];
  return Object.entries(value).filter(
    (entry): entry is [string, string[]] =>
      entry[0].length > 0 &&
      Array.isArray(entry[1]) &&
      entry[1].every((line) => typeof line === "string"),
  );
}

function safeCategoryPart(value: string): boolean {
  return (
    value.length > 0 &&
    value !== "." &&
    value !== ".." &&
    !/[\\/:*?"<>|\0]/u.test(value)
  );
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function tomlString(value: string): string {
  return JSON.stringify(value)
    .replaceAll("\\b", "\\u0008")
    .replaceAll("\\f", "\\u000c");
}
