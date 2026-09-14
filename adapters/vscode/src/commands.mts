import * as vscode from "vscode";

import type {
  CommandResult,
  ExecuteCommandParams,
  GetNoteOptionsParams,
  GetNoteOptionsResult,
  PreviewTarget,
} from "./protocol.mjs";
import { selectWorkspaceUriWithPlaceholder } from "./workspace-selection.mjs";

export async function executeCreateNote(
  params: ExecuteCommandParams,
  getOptions: (params: GetNoteOptionsParams) => Promise<GetNoteOptionsResult>,
  pick: (
    items: readonly string[],
    options: { placeHolder: string },
  ) => Thenable<string | undefined>,
  execute: (params: ExecuteCommandParams) => Promise<CommandResult>,
): Promise<CommandResult | undefined> {
  if (!params.workspaceUri) return execute(params);
  const options = await getOptions({
    daemonInstanceId: params.daemonInstanceId,
    workspaceUri: params.workspaceUri,
  });
  const noteCategory = options.categories.length
    ? await pick(options.categories, {
        placeHolder: vscode.l10n.t("Select a note category"),
      })
    : undefined;
  if (options.categories.length && noteCategory === undefined) return;
  const noteTemplate = options.templates.length
    ? await pick(options.templates, {
        placeHolder: vscode.l10n.t("Select a note template"),
      })
    : undefined;
  if (options.templates.length && noteTemplate === undefined) return;
  return execute({ ...params, noteCategory, noteTemplate });
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
