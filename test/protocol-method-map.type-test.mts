import type {
  GetNoteOptionsResult,
  InitializeResult,
} from "../adapters/vscode/src/protocol.mjs";
import type { JsonRpcConnection } from "../adapters/vscode/src/rpc.mjs";

type Equal<Left, Right> =
  (<Value>() => Value extends Left ? 1 : 2) extends <
    Value,
  >() => Value extends Right ? 1 : 2
    ? true
    : false;
type Assert<Value extends true> = Value;

declare const connection: JsonRpcConnection;

const initialize = connection.request("fleximark/initialize", {
  protocolVersion: 2,
  client: { name: "type-test", version: "1" },
});
type InitializeIsInferred = Assert<
  Equal<typeof initialize, Promise<InitializeResult>>
>;
type InitializeVersionIsLiteral = Assert<
  Equal<InitializeResult["protocolVersion"], 2>
>;

const noteOptions = connection.request("fleximark/getNoteOptions", {
  daemonInstanceId: "daemon",
  workspaceUri: "file:///workspace",
});
type NoteOptionsAreInferred = Assert<
  Equal<typeof noteOptions, Promise<GetNoteOptionsResult>>
>;

connection.request("fleximark/getNoteOptions", {
  daemonInstanceId: "daemon",
  // @ts-expect-error getNoteOptions requires workspaceUri, not command.
  command: "editTheme",
});

// @ts-expect-error requestFullText is daemon-to-client, not a request.
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

void (null as unknown as InitializeIsInferred);
void (null as unknown as InitializeVersionIsLiteral);
void (null as unknown as NoteOptionsAreInferred);
void initialize;
void noteOptions;
