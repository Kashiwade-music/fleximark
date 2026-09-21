export interface LegacyWorkspaceSettings {
  readonly defaultPreviewMode?: unknown;
  readonly noteCategories?: unknown;
  readonly noteFileNamePrefix?: unknown;
  readonly noteFileNameSuffix?: unknown;
  readonly noteTemplates?: unknown;
}

export interface LegacyWorkspaceState {
  readonly hasLegacyPlugin: boolean;
  readonly settings: LegacyWorkspaceSettings;
}

export interface WorkspaceMigrationDependencies<Workspace> {
  key(workspace: Workspace): string;
  detect(workspace: Workspace): Promise<LegacyWorkspaceState | undefined>;
  confirm(workspace: Workspace, state: LegacyWorkspaceState): Promise<boolean>;
  migrate(workspace: Workspace, state: LegacyWorkspaceState): Promise<void>;
  completed(workspace: Workspace, state: LegacyWorkspaceState): Promise<void>;
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
    await this.#dependencies.migrate(workspace, state);
    await this.#dependencies.completed(workspace, state);
  }

  forget(workspace: Workspace): void {
    this.#handled.delete(this.#dependencies.key(workspace));
  }
}
