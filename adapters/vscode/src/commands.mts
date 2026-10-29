import { WORKSPACE_COMMAND_KIND } from "@fleximark/contracts/protocol";
import {
  COMMAND_DESCRIPTORS,
  type CommandResult,
  type GetNoteOptionsResult,
  type MigrationPlanResult,
  type NoteCategoryOption,
  type PreviewTarget,
  type WorkspaceCommand,
  type WorkspaceCommandName,
  type WorkspaceCommandRequest,
  commandDescriptor,
} from "@fleximark/contracts/protocol";
import * as vscode from "vscode";

export function buildWorkspaceOperation(
  kind: WorkspaceCommandName,
  sourceOffset?: number,
): WorkspaceCommand {
  const descriptor = commandDescriptor(kind);
  switch (descriptor.kind) {
    case WORKSPACE_COMMAND_KIND.planMigration:
      return { command: descriptor.name, recover: false };
    case WORKSPACE_COMMAND_KIND.openPreview:
      return {
        command: descriptor.name,
        target: "externalBrowser",
        ...(sourceOffset === undefined ? {} : { sourceOffset }),
      };
    case WORKSPACE_COMMAND_KIND.applyMigration:
      throw new Error(vscode.l10n.t("Plan the migration before applying it."));
    default:
      return { command: descriptor.name };
  }
}

export async function executeCreateNote(
  params: WorkspaceCommandRequest,
  getOptions: (
    params: WorkspaceCommandRequest,
  ) => Promise<GetNoteOptionsResult>,
  pickCategory: (
    items: readonly NoteCategoryQuickPickItem[],
    options: { placeHolder: string },
  ) => Thenable<NoteCategoryQuickPickItem | undefined>,
  pickTemplate: (
    items: readonly string[],
    options: { placeHolder: string },
  ) => Thenable<string | undefined>,
  promptFileName: (options: {
    prompt: string;
    validateInput(value: string): string | undefined;
  }) => Thenable<string | undefined>,
  execute: (params: WorkspaceCommandRequest) => Promise<CommandResult>,
): Promise<CommandResult | undefined> {
  if (!params.context.workspaceUri) return execute(params);
  const options = await getOptions({
    daemonInstanceId: params.daemonInstanceId,
    context: params.context,
    operation: {
      command: commandDescriptor(WORKSPACE_COMMAND_KIND.getNoteOptions).name,
    },
  });
  const noteCategoryPath = options.categories.length
    ? await selectNoteCategory(options.categories, pickCategory)
    : undefined;
  if (options.categories.length && noteCategoryPath === undefined) return;
  const noteTemplate = options.templates.length
    ? await pickTemplate(options.templates, {
        placeHolder: vscode.l10n.t("Select a note template"),
      })
    : undefined;
  if (options.templates.length && noteTemplate === undefined) return;
  const noteFileName = await promptFileName({
    prompt: vscode.l10n.t("Enter a file name"),
    validateInput: (value) =>
      value.trim() ? undefined : vscode.l10n.t("File name cannot be empty"),
  });
  if (noteFileName === undefined) return;
  const commandParams: WorkspaceCommandRequest = {
    ...params,
    operation: {
      command: commandDescriptor(WORKSPACE_COMMAND_KIND.createNote).name,
      ...(noteCategoryPath ? { categoryPath: noteCategoryPath } : {}),
      ...(noteTemplate ? { template: noteTemplate } : {}),
      name: noteFileName,
    },
  };
  return execute(commandParams);
}

export async function executeMigration(
  params: WorkspaceCommandRequest,
  execute: (params: WorkspaceCommandRequest) => Promise<CommandResult>,
  confirm: (plan: MigrationPlanResult) => PromiseLike<boolean>,
  isCurrent: () => boolean,
): Promise<CommandResult | undefined> {
  const result = await execute({
    ...params,
    operation: {
      command: commandDescriptor(WORKSPACE_COMMAND_KIND.planMigration).name,
      recover: false,
    },
  });
  if (!isCurrent()) return;
  if (result.kind !== "migrationPlanned")
    throw new Error(vscode.l10n.t("Invalid migration plan response"));
  if (result.plan.status !== "ready") return result;
  if (!(await confirm(result.plan)) || !isCurrent()) return;
  return execute({
    ...params,
    operation: {
      command: commandDescriptor(WORKSPACE_COMMAND_KIND.applyMigration).name,
      planId: result.plan.planId,
    },
  });
}

export async function confirmMigrationPlan(
  plan: MigrationPlanResult,
): Promise<boolean> {
  const apply = vscode.l10n.t("Apply migration");
  const detail = [
    plan.status,
    ...plan.outputs,
    ...plan.warnings,
    plan.config,
  ].join("\n");
  return (
    (await vscode.window.showWarningMessage(
      vscode.l10n.t("Review the workspace migration plan."),
      { modal: true, detail },
      apply,
    )) === apply
  );
}

export async function showMigrationPlanStatus(
  plan: MigrationPlanResult,
): Promise<void> {
  await vscode.window.showInformationMessage(
    vscode.l10n.t("Workspace migration status: {0}", plan.status),
    {
      modal: true,
      detail: [
        plan.status,
        ...plan.outputs,
        ...plan.warnings,
        plan.config,
      ].join("\n"),
    },
  );
}

export interface NoteCategoryQuickPickItem extends vscode.QuickPickItem {
  action: "open" | "select" | "useCurrent" | "back";
  category?: NoteCategoryOption;
}

export async function selectNoteCategory(
  categories: readonly NoteCategoryOption[],
  pick: (
    items: readonly NoteCategoryQuickPickItem[],
    options: { placeHolder: string },
  ) => Thenable<NoteCategoryQuickPickItem | undefined>,
): Promise<string[] | undefined> {
  const stack: NoteCategoryOption[] = [];
  let current = categories;
  for (;;) {
    const parent = stack.at(-1);
    const items: NoteCategoryQuickPickItem[] = [];
    if (parent) {
      items.push({
        label: `$(check) ${vscode.l10n.t("Use this category")}`,
        description: stack.map((category) => category.name).join(" / "),
        action: "useCurrent",
        category: parent,
      });
    }
    items.push(
      ...current.map((category) => ({
        label: category.name,
        description: [
          ...stack.map((parent) => parent.name),
          category.name,
        ].join(" / "),
        action: category.children.length
          ? ("open" as const)
          : ("select" as const),
        category,
      })),
    );
    if (parent) {
      items.push({
        label: `$(arrow-left) ${vscode.l10n.t("Back")}`,
        action: "back",
      });
    }
    const breadcrumb = stack.map((category) => category.name).join(" / ");
    const selected = await pick(items, {
      placeHolder: breadcrumb
        ? vscode.l10n.t("Select a note category in {0}", breadcrumb)
        : vscode.l10n.t("Select a note category"),
    });
    if (!selected) return undefined;
    if (selected.action === "back") {
      stack.pop();
      current = stack.at(-1)?.children ?? categories;
      continue;
    }
    if (selected.action === "useCurrent") {
      return stack.map((category) => category.name);
    }
    const category = selected.category;
    if (!category) return undefined;
    if (selected.action === "select") {
      return [...stack.map((parent) => parent.name), category.name];
    }
    stack.push(category);
    current = category.children;
  }
}

export async function openCommandResult(
  params: WorkspaceCommandRequest,
  result: CommandResult,
  openUri: (uri: string) => Promise<void>,
  execute: (params: WorkspaceCommandRequest) => Promise<CommandResult>,
): Promise<void> {
  const uri =
    result.kind === "htmlExported"
      ? result.outputUri
      : result.kind === "themeEdited" ||
          result.kind === "noteCreated" ||
          result.kind === "admonitionsCollected"
        ? result.uri
        : result.kind === "workspaceInitialized"
          ? result.configUri
          : undefined;
  if (!uri) return;
  await openUri(uri);
  if (result.kind === "htmlExported")
    await execute({
      ...params,
      operation: {
        command: commandDescriptor(WORKSPACE_COMMAND_KIND.acknowledgeExport)
          .name,
        destinationUri: result.outputUri,
      },
    });
}

interface CommandAdapter {
  migrateWorkspace?(): Promise<void>;
  execute(command: string): Promise<void>;
  forceReload(): Promise<void>;
  openPreview(target: PreviewTarget): Promise<void>;
  report(error: unknown): void;
}

export type CommandRegistrar = Pick<typeof vscode.commands, "registerCommand">;

export function registerCommands(
  adapter: CommandAdapter,
  registrar: CommandRegistrar = vscode.commands,
  isActive: () => boolean = () => true,
): vscode.Disposable[] {
  const guarded =
    (action: () => void | Promise<void>, reportFailure: boolean) => () => {
      if (!isActive()) return Promise.resolve();
      return Promise.resolve(action()).catch((error: unknown) => {
        if (isActive() && reportFailure) adapter.report(error);
        throw error;
      });
    };
  const run = (action: () => void | Promise<void>) => guarded(action, true);
  return [
    registrar.registerCommand(
      "fleximark.migrate",
      run(() => adapter.migrateWorkspace?.()),
    ),
    registrar.registerCommand(
      "fleximark.previewMarkdown",
      run(() => {
        const target = vscode.workspace
          .getConfiguration("fleximark")
          .get<PreviewTarget>("previewTarget", "embeddedHtml");
        return adapter.openPreview(target);
      }),
    ),
    registrar.registerCommand(
      "fleximark.previewMarkdownOnVscode",
      run(() => adapter.openPreview("embeddedHtml")),
    ),
    registrar.registerCommand(
      "fleximark.previewMarkdownOnBrowser",
      run(() => adapter.openPreview("externalBrowser")),
    ),
    registrar.registerCommand(
      "fleximark.forceReloadPreview",
      guarded(() => adapter.forceReload(), false),
    ),
    ...["workspace", "previewAt"].map((command) =>
      registrar.registerCommand(
        `fleximark.${command}`,
        run(() => adapter.execute(command)),
      ),
    ),
    ...COMMAND_DESCRIPTORS.filter(
      (descriptor) =>
        descriptor.codeAction &&
        descriptor.browserUi &&
        descriptor.kind !== WORKSPACE_COMMAND_KIND.planMigration,
    ).map((descriptor) =>
      registrar.registerCommand(
        `fleximark.${descriptor.name}`,
        run(() => adapter.execute(descriptor.name)),
      ),
    ),
  ];
}
