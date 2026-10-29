import type {
  CommandResult,
  MigrationPlanResult,
  NoteCategoryOption,
  WorkspaceStateResult,
  WorkspaceUiCommandRequest,
} from "../../contracts/typescript/protocol.generated.mjs";
import { COMMAND_DESCRIPTORS } from "../../contracts/typescript/protocol.generated.mjs";
import { WORKSPACE_COMMAND_KIND } from "../../contracts/typescript/protocol.mjs";
import { hasContractShape } from "../../contracts/typescript/protocol.mjs";

export const browserCommands = COMMAND_DESCRIPTORS.filter(
  (command) => command.browserUi,
);
const object = (v: unknown): v is Record<string, unknown> =>
  v !== null && typeof v === "object" && !Array.isArray(v);
const text = (v: unknown): v is string => typeof v === "string";
export function isWorkspaceState(v: unknown): v is WorkspaceStateResult {
  return hasContractShape("workspaceStateResult", v);
}
export function isCommandResult(v: unknown): v is CommandResult {
  return hasContractShape("commandResult", v);
}

export async function runWorkspaceCommand(
  input: WorkspaceUiCommandRequest,
  send: (input: WorkspaceUiCommandRequest) => Promise<CommandResult>,
  confirmMigration: (plan: MigrationPlanResult) => boolean | Promise<boolean>,
  isCurrent: () => boolean = () => true,
): Promise<CommandResult | undefined> {
  if (!browserCommands.some(({ name }) => name === input.operation.command))
    throw new Error("This operation is not available in the browser.");
  const response = await send(input);
  if (!isCurrent()) return;
  if (response.kind !== "migrationPlanned") return response;
  if (response.plan.status !== "ready") return response;
  if (!(await confirmMigration(response.plan)) || !isCurrent()) return;
  const descriptor = COMMAND_DESCRIPTORS.find(
    ({ kind }) => kind === WORKSPACE_COMMAND_KIND.applyMigration,
  );
  if (!descriptor || descriptor.kind !== WORKSPACE_COMMAND_KIND.applyMigration)
    throw new Error("Migration operation is unavailable.");
  return send({
    context: input.context,
    operation: { command: descriptor.name, planId: response.plan.planId },
  });
}

export function migrationSummary(plan: MigrationPlanResult): string {
  return [
    `Status: ${plan.status}`,
    `Workspace: ${plan.workspaceUri}`,
    "Outputs:",
    ...plan.outputs,
    "Warnings:",
    ...plan.warnings,
    "Configuration:",
    plan.config,
  ].join("\n");
}
export function categoryPaths(
  categories: NoteCategoryOption[],
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
      object(value) && text(value.error)
        ? value.error
        : "Request failed (" + response.status + ").",
    );
  if (!validate(value))
    throw new Error("The server returned an invalid response.");
  return value;
}
