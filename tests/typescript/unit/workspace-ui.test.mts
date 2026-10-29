import { WORKSPACE_COMMAND_KIND } from "@fleximark/contracts/protocol";
import {
  COMMAND_DESCRIPTORS,
  type CommandResult,
  type WorkspaceUiCommandRequest,
} from "@fleximark/contracts/protocol.generated";
import {
  browserCommands,
  categoryPaths,
  isCommandResult,
  isWorkspaceState,
  migrationSummary,
  readResponse,
  runWorkspaceCommand,
  validateFileName,
} from "@fleximark/workspace-ui/model";
import * as assert from "node:assert/strict";

export const suiteName = "Shared workspace workflow";
export function suite(): void {
  test("preserves selectable intermediate and nested category paths", () => {
    assert.deepEqual(
      categoryPaths([
        { name: "Work", children: [{ name: "Notes", children: [] }] },
      ]),
      [["Work"], ["Work", "Notes"]],
    );
  });
  test("rejects incomplete server context and malformed note options", () => {
    assert.equal(
      isWorkspaceState({ context: {}, workspaces: [], documents: [] }),
      true,
    );
    assert.equal(
      isWorkspaceState({
        context: {},
        workspaces: [{ uri: "file:///work" }],
        documents: [],
      }),
      false,
    );
    assert.equal(
      isWorkspaceState({
        context: {},
        workspaces: [],
        documents: [{ uri: "file:///a", version: 1.5 }],
      }),
      false,
    );
    assert.equal(
      isWorkspaceState({
        context: {},
        workspaces: [],
        documents: [],
        noteOptions: {
          categories: [{ name: "x", children: "bad" }],
          templates: [],
        },
      }),
      false,
    );
  });
  test("validates note names before sending a command", () => {
    for (const name of [
      "",
      " ",
      "../secret",
      "folder/note",
      "..",
      "../..",
      "a".repeat(256),
    ])
      assert.equal(validateFileName(name), false, name);
    assert.equal(validateFileName("Meeting notes.md"), true);
  });
  test("surfaces server errors and rejects invalid success payloads", async () => {
    const response = (value: unknown, status = 200) =>
      ({ ok: status === 200, status, json: async () => value }) as Response;
    await assert.rejects(
      readResponse(
        response({ error: "Document changed; select it again" }, 409),
        isCommandResult,
      ),
      /Document changed/,
    );
    await assert.rejects(
      readResponse(response({ openUri: 42 }), isCommandResult),
      /invalid response/,
    );
  });
  test("uses browser permissions from generated descriptors including export acknowledgement", () => {
    assert.deepEqual(
      browserCommands,
      COMMAND_DESCRIPTORS.filter(({ browserUi }) => browserUi),
    );
    assert.ok(
      browserCommands.some(
        ({ kind }) => kind === WORKSPACE_COMMAND_KIND.acknowledgeExport,
      ),
    );
    assert.equal(
      isCommandResult({ message: "old form", openUri: "file:///out" }),
      false,
    );
    assert.equal(isWorkspaceState({ workspaces: [], documents: [] }), false);
  });
  test("migration shows the complete plan before confirmation and applies its exact plan id", async () => {
    const calls: WorkspaceUiCommandRequest[] = [];
    const plan: Extract<CommandResult, { kind: "migrationPlanned" }> = {
      kind: "migrationPlanned",
      plan: {
        planId: "plan-1",
        workspaceUri: "file:///work",
        status: "ready",
        outputs: ["note.md"],
        warnings: ["old plugin omitted"],
        config: "schemaVersion = 1",
        recovery: false,
      },
    };
    const planning = COMMAND_DESCRIPTORS.find(
      ({ kind }) => kind === WORKSPACE_COMMAND_KIND.planMigration,
    );
    const applying = COMMAND_DESCRIPTORS.find(
      ({ kind }) => kind === WORKSPACE_COMMAND_KIND.applyMigration,
    );
    assert.ok(
      planning &&
        planning.kind === WORKSPACE_COMMAND_KIND.planMigration &&
        applying &&
        applying.kind === WORKSPACE_COMMAND_KIND.applyMigration,
    );
    const input: WorkspaceUiCommandRequest = {
      context: { workspaceUri: "file:///work" },
      operation: { command: planning.name, recover: false },
    };
    const send = async (
      request: WorkspaceUiCommandRequest,
    ): Promise<CommandResult> => {
      calls.push(request);
      return calls.length === 1
        ? plan
        : { kind: "migrationApplied", backupUri: null, recovered: false };
    };
    const response = await runWorkspaceCommand(input, send, (received) => {
      assert.equal(calls.length, 1);
      assert.equal(received, plan.plan);
      const summary = migrationSummary(received);
      for (const text of [
        "ready",
        "note.md",
        "old plugin omitted",
        "schemaVersion = 1",
      ])
        assert.ok(summary.includes(text));
      return true;
    });
    assert.equal(response?.kind, "migrationApplied");
    assert.deepEqual(calls[1], {
      context: input.context,
      operation: { command: applying.name, planId: "plan-1" },
    });
    calls.length = 0;
    assert.equal(
      await runWorkspaceCommand(input, send, () => false),
      undefined,
    );
    assert.equal(calls.length, 1);
    calls.length = 0;
    let current = true;
    assert.equal(
      await runWorkspaceCommand(
        input,
        send,
        () => {
          current = false;
          return true;
        },
        () => current,
      ),
      undefined,
    );
    assert.equal(calls.length, 1);
    calls.length = 0;
    let confirmations = 0;
    assert.equal(
      await runWorkspaceCommand(
        input,
        send,
        () => {
          confirmations++;
          return true;
        },
        () => false,
      ),
      undefined,
    );
    assert.equal(confirmations, 0);
    assert.equal(calls.length, 1);
  });
}
