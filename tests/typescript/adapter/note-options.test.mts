import * as assert from "node:assert/strict";

import {
  executeCreateNote,
  selectNoteCategory,
} from "../../../adapters/vscode/src/adapter.mjs";
import { executeMigration } from "../../../adapters/vscode/src/commands.mjs";
import { WORKSPACE_COMMAND_KIND } from "../../../contracts/typescript/protocol.mjs";
import type {
  CommandResult,
  MigrationPlanResult,
  WorkspaceCommandRequest,
} from "../../../contracts/typescript/protocol.mjs";
import { customRequestParamsValidators } from "../../../contracts/typescript/protocol.mjs";

export const suiteName = "Note option adapter";

export function suite(): void {
  test("applies only a confirmed migration plan on the current connection", async () => {
    const plan: MigrationPlanResult = {
      planId: "plan",
      workspaceUri: "file:///workspace",
      status: "ready",
      outputs: ["notes/note.md"],
      warnings: ["Old plugins are not migrated"],
      config: "schemaVersion = 1",
      recovery: false,
    };
    const base: WorkspaceCommandRequest = {
      daemonInstanceId: "daemon",
      context: { workspaceUri: plan.workspaceUri },
      operation: {
        command: WORKSPACE_COMMAND_KIND.planMigration,
        recover: false,
      },
    };
    for (const scenario of [
      "confirmed",
      "cancelled",
      "stalePlan",
      "staleConfirmation",
      "alreadyCurrent",
      "recoveryRequired",
    ] as const) {
      const requests: WorkspaceCommandRequest[] = [];
      let current = true;
      let confirmations = 0;
      const status =
        scenario === "alreadyCurrent" || scenario === "recoveryRequired"
          ? scenario
          : "ready";
      const result = await executeMigration(
        base,
        async (request): Promise<CommandResult> => {
          requests.push(request);
          if (
            request.operation.command === WORKSPACE_COMMAND_KIND.planMigration
          ) {
            if (scenario === "stalePlan") current = false;
            return { kind: "migrationPlanned", plan: { ...plan, status } };
          }
          assert.deepEqual(request.operation, {
            command: WORKSPACE_COMMAND_KIND.applyMigration,
            planId: plan.planId,
          });
          assert.deepEqual(request.context, base.context);
          assert.equal(request.daemonInstanceId, base.daemonInstanceId);
          return {
            kind: "migrationApplied",
            backupUri: null,
            recovered: false,
          };
        },
        async (shown) => {
          confirmations += 1;
          assert.deepEqual(shown, plan);
          if (scenario === "staleConfirmation") current = false;
          return scenario !== "cancelled";
        },
        () => current,
      );
      assert.equal(requests.length, scenario === "confirmed" ? 2 : 1, scenario);
      assert.equal(
        confirmations,
        ["stalePlan", "alreadyCurrent", "recoveryRequired"].includes(scenario)
          ? 0
          : 1,
        scenario,
      );
      if (scenario === "confirmed")
        assert.equal(result?.kind, "migrationApplied");
    }
  });
  test("prompts for a named note after selecting Japanese categories without templates", async () => {
    const base: WorkspaceCommandRequest = {
      daemonInstanceId: "daemon-1",
      context: { workspaceUri: "file:///workspace" },
      operation: { command: WORKSPACE_COMMAND_KIND.createNote },
    };
    let step = 0;
    let prompted = false;
    let executed: WorkspaceCommandRequest | undefined;
    await executeCreateNote(
      base,
      async () => ({
        categories: [
          {
            name: "DTM関連",
            children: [
              { name: "DTM入門", children: [] },
              { name: "その他", children: [] },
            ],
          },
        ],
        templates: [],
      }),
      async (items) => {
        const category = step++ === 0 ? "DTM関連" : "その他";
        return items.find((item) => item.category?.name === category);
      },
      async () => {
        throw new Error("no template picker without templates");
      },
      async () => {
        assert.equal(step, 2);
        prompted = true;
        return "ミックス";
      },
      async (params) => {
        assert.equal(prompted, true);
        executed = params;
        return { kind: "noteCreated", uri: "file:///workspace/note.md" };
      },
    );
    assert.deepEqual(executed, {
      ...base,
      operation: {
        command: WORKSPACE_COMMAND_KIND.createNote,
        categoryPath: ["DTM関連", "その他"],
        name: "ミックス",
      },
    });
    assert.equal(
      customRequestParamsValidators["fleximark/executeCommand"](executed),
      true,
    );
  });

  test("executes selected note options and stops after cancellation", async () => {
    const base: WorkspaceCommandRequest = {
      daemonInstanceId: "daemon-1",
      context: { workspaceUri: "file:///workspace" },
      operation: { command: WORKSPACE_COMMAND_KIND.createNote },
    };
    let requested: WorkspaceCommandRequest | undefined;
    let categoryStep = 0;
    let executed: WorkspaceCommandRequest | undefined;
    await executeCreateNote(
      base,
      async (params) => {
        requested = params;
        return {
          categories: [
            {
              name: "Work",
              children: [{ name: "Project", children: [] }],
            },
            { name: "Personal", children: [] },
          ],
          templates: ["meeting", "blank"],
        };
      },
      async (items) => {
        const name = categoryStep++ === 0 ? "Work" : "Project";
        return items.find((item) => item.category?.name === name);
      },
      async () => "meeting",
      async (options) => {
        assert.equal(options.validateInput(""), "File name cannot be empty");
        assert.equal(options.validateInput("session"), undefined);
        return "session";
      },
      async (params) => {
        executed = params;
        return { kind: "noteCreated", uri: "file:///workspace/note.md" };
      },
    );
    assert.deepEqual(requested, {
      daemonInstanceId: "daemon-1",
      context: { workspaceUri: "file:///workspace" },
      operation: { command: WORKSPACE_COMMAND_KIND.getNoteOptions },
    });
    assert.deepEqual(executed, {
      ...base,
      operation: {
        command: WORKSPACE_COMMAND_KIND.createNote,
        categoryPath: ["Work", "Project"],
        template: "meeting",
        name: "session",
      },
    });
    let executedAfterCancellation = false;
    const result = await executeCreateNote(
      base,
      async () => ({
        categories: [{ name: "Work", children: [] }],
        templates: ["blank"],
      }),
      async () => undefined,
      async () => "blank",
      async () => "cancelled-before-file-name",
      async () => {
        executedAfterCancellation = true;
        return { kind: "noteCreated", uri: "file:///workspace/note.md" };
      },
    );
    assert.equal(result, undefined);
    assert.equal(executedAfterCancellation, false);

    const fileNameCancellation = await executeCreateNote(
      base,
      async () => ({ categories: [], templates: [] }),
      async () => undefined,
      async () => undefined,
      async () => undefined,
      async () => {
        throw new Error("cancelled file name must not execute");
      },
    );
    assert.equal(fileNameCancellation, undefined);

    let migratedWorkspaceParams: WorkspaceCommandRequest | undefined;
    await executeCreateNote(
      {
        ...base,
        context: { workspaceUri: "file:///workspace" },
      },
      async () => ({
        categories: [{ name: "Work", children: [] }],
        templates: ["daily"],
      }),
      async (items) => items.find((item) => item.category?.name === "Work"),
      async () => "daily",
      async () => "legacy-note",
      async (params) => {
        migratedWorkspaceParams = params;
        return { kind: "noteCreated", uri: "file:///workspace/note.md" };
      },
    );
    assert.deepEqual(migratedWorkspaceParams, {
      ...base,
      operation: {
        command: WORKSPACE_COMMAND_KIND.createNote,
        categoryPath: ["Work"],
        template: "daily",
        name: "legacy-note",
      },
    });
    assert.equal(
      customRequestParamsValidators["fleximark/executeCommand"](
        migratedWorkspaceParams,
      ),
      true,
    );
  });

  test("supports choosing a parent and returns breadcrumb paths", async () => {
    const categories = [
      {
        name: "Left",
        children: [{ name: "Same", children: [] }],
      },
      { name: "Right", children: [{ name: "Same", children: [] }] },
    ];
    let step = 0;
    const selected = await selectNoteCategory(categories, async (items) => {
      if (step++ === 0) {
        assert.deepEqual(
          items.map((item) => item.description),
          ["Left", "Right"],
        );
        return items.find((item) => item.category?.name === "Left");
      }
      assert.deepEqual(
        items.map((item) => item.description),
        ["Left", "Left / Same", undefined],
      );
      return items.find((item) => item.action === "useCurrent");
    });
    assert.deepEqual(selected, ["Left"]);

    let rightStep = 0;
    const duplicateName = await selectNoteCategory(
      categories,
      async (items) => {
        const name = rightStep++ === 0 ? "Right" : "Same";
        return items.find(
          (item) =>
            item.category?.name === name && item.action !== "useCurrent",
        );
      },
    );
    assert.deepEqual(duplicateName, ["Right", "Same"]);
  });
}
