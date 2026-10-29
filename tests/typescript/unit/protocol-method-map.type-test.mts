import { WORKSPACE_COMMAND_KIND } from "@fleximark/contracts/protocol";
import {
  clientNotificationValidators,
  customRequestResultValidators,
  hasContractShape,
  isPreviewNavigationEvent,
  isRenderFrame,
  isRenderNavigationEvent,
} from "@fleximark/contracts/protocol";
import type {
  CommandResult,
  InitializeResult,
} from "@fleximark/contracts/protocol";
import type { JsonRpcConnection } from "fleximark/src/rpc";

type Equal<Left, Right> =
  (<Value>() => Value extends Left ? 1 : 2) extends <
    Value,
  >() => Value extends Right ? 1 : 2
    ? true
    : false;
type Assert<Value extends true> = Value;

declare const connection: JsonRpcConnection;
declare const untrusted: unknown;

if (hasContractShape("initializeResult", untrusted)) {
  const version: typeof import("@fleximark/contracts/protocol.generated").PROTOCOL_VERSION =
    untrusted.protocolVersion;
  void version;
}
if (isRenderFrame(untrusted)) {
  const revision: number = untrusted.renderRevision;
  // @ts-expect-error validated frames have no old plugin annotations.
  void untrusted.annotations;
  void revision;
}
if (isPreviewNavigationEvent(untrusted)) {
  const direction: "selectNode" | "revealNode" = untrusted.type;
  void direction;
}
if (isRenderNavigationEvent(untrusted)) {
  const direction: "selection" | "viewport" = untrusted.type;
  void direction;
}
if (customRequestResultValidators["fleximark/executeCommand"](untrusted)) {
  const result: CommandResult = untrusted;
  void result;
}
if (clientNotificationValidators["fleximark/setSelection"](untrusted)) {
  const session: string = untrusted.documentSessionId;
  void session;
}

type InitializeVersionIsLiteral = Assert<
  Equal<
    InitializeResult["protocolVersion"],
    typeof import("@fleximark/contracts/protocol").PROTOCOL_VERSION
  >
>;

const noteOptions = connection.request("fleximark/executeCommand", {
  daemonInstanceId: "daemon",
  context: { workspaceUri: "file:///workspace" },
  operation: { command: WORKSPACE_COMMAND_KIND.getNoteOptions },
});
type NoteOptionsAreInferred = Assert<
  Equal<typeof noteOptions, Promise<CommandResult>>
>;

connection.request("fleximark/executeCommand", {
  daemonInstanceId: "daemon",
  context: {},
  operation: {
    command: WORKSPACE_COMMAND_KIND.createNote,
    // @ts-expect-error createNote does not accept export destinations.
    destinationUri: "file:///output",
  },
});

// @ts-expect-error getNoteOptions is a command, not a protocol method.
connection.request("fleximark/getNoteOptions", {});

// @ts-expect-error the removed recovery method is outside the current protocol.
connection.request("fleximark/requestFullText", {
  daemonInstanceId: "daemon",
  uri: "file:///document.md",
  documentSessionId: "document",
  reason: "contentHashMismatch",
});

connection.notify("fleximark/setSelection", {
  daemonInstanceId: "daemon",
  documentSessionId: "document",
  expectedDocumentVersion: 1,
  // @ts-expect-error selection positions cannot be null.
  selections: [{ anchor: null, active: { line: 0, character: 0 } }],
});

// @ts-expect-error custom methods cannot use the LSP request escape hatch.
connection.requestLsp("fleximark/render", {});
// @ts-expect-error custom methods cannot use the LSP notification escape hatch.
connection.notifyLsp("fleximark/previewEvent", {});

void (null as unknown as InitializeVersionIsLiteral);
void (null as unknown as NoteOptionsAreInferred);
void noteOptions;
