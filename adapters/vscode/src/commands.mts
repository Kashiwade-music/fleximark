import * as vscode from "vscode";

import type {
  CommandResult,
  ExecuteCommandParams,
  GetNoteOptionsParams,
  GetNoteOptionsResult,
  NoteCategoryOption,
  PreviewTarget,
} from "./protocol.mjs";
import { selectWorkspaceUriWithPlaceholder } from "./workspace-selection.mjs";

export async function executeCreateNote(
  params: ExecuteCommandParams,
  getOptions: (params: GetNoteOptionsParams) => Promise<GetNoteOptionsResult>,
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
  execute: (params: ExecuteCommandParams) => Promise<CommandResult>,
): Promise<CommandResult | undefined> {
  if (!params.workspaceUri) return execute(params);
  const options = await getOptions({
    daemonInstanceId: params.daemonInstanceId,
    workspaceUri: params.workspaceUri,
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
  const commandParams: ExecuteCommandParams = {
    ...params,
    noteFileName,
  };
  if (noteCategoryPath !== undefined)
    commandParams.noteCategoryPath = noteCategoryPath;
  if (noteTemplate !== undefined) commandParams.noteTemplate = noteTemplate;
  return execute(commandParams);
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
  params: ExecuteCommandParams,
  result: CommandResult,
  openUri: (uri: string) => Promise<void>,
  execute: (params: ExecuteCommandParams) => Promise<CommandResult>,
): Promise<void> {
  if (!result.openUri) return;
  await openUri(result.openUri);
  if (params.command === "exportHtml")
    await execute({ ...params, command: "acknowledgeExport" });
}

export async function selectWorkspaceUri(
  activeWorkspaceUri: string | undefined,
  workspaces: readonly { label: string; uri: string }[],
  pick: (
    items: readonly { label: string; uri: string }[],
    options: { placeHolder: string },
  ) => Thenable<{ label: string; uri: string } | undefined>,
): Promise<string | undefined> {
  return selectWorkspaceUriWithPlaceholder(
    activeWorkspaceUri,
    workspaces,
    pick,
    vscode.l10n.t("Select a FlexiMark workspace"),
  );
}

interface CommandAdapter {
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
      "fleximark.previewMarkdown",
      run(() => {
        const target = vscode.workspace
          .getConfiguration("fleximark")
          .get<"embeddedHtml" | "externalBrowser">(
            "previewTarget",
            "embeddedHtml",
          );
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
    ...[
      "exportHtml",
      "createNote",
      "initializeWorkspace",
      "collectAdmonitions",
      "editTheme",
    ].map((command) =>
      registrar.registerCommand(
        `fleximark.${command}`,
        run(() => adapter.execute(command)),
      ),
    ),
  ];
}
