import type { JsonRpcConnection } from "../../adapters/vscode/src/rpc.mjs";
import { WORKSPACE_COMMAND_KIND } from "../../contracts/typescript/protocol.mjs";
import type {
  CommandResult,
  InitializeResult,
} from "../../contracts/typescript/protocol.mjs";

type Equal<Left, Right> =
  (<Value>() => Value extends Left ? 1 : 2) extends <
    Value,
  >() => Value extends Right ? 1 : 2
    ? true
    : false;
type Assert<Value extends true> = Value;

declare const connection: JsonRpcConnection;

type InitializeVersionIsLiteral = Assert<
  Equal<
    InitializeResult["protocolVersion"],
    typeof import("../../contracts/typescript/protocol.mjs").PROTOCOL_VERSION
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
