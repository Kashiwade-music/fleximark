/** Browser-neutral consumers of the Rust-owned FlexiMark wire contract. */
import { validateContractSchema } from "./contract-validator.mjs";
import { isRecord } from "./guards.mjs";
import {
  CLIENT_NOTIFICATION_VALIDATORS,
  CONTRACT_DEFINITIONS,
  COMMAND_DESCRIPTORS as ContractCommands,
  REQUEST_VALIDATORS,
  SERVER_NOTIFICATION_VALIDATORS,
  TYPE_VALIDATORS,
} from "./protocol.generated.mjs";
import type * as Contract from "./protocol.generated.mjs";
import { semanticHandlers } from "./semantic-validation.mjs";

export {
  WORKSPACE_COMMAND_KIND,
  PROTOCOL_VERSION,
  COMMAND_DESCRIPTORS,
  SEMANTIC_TOKEN_TYPES,
  SEMANTIC_TOKEN_MODIFIERS,
} from "./protocol.generated.mjs";
export type * from "./protocol.generated.mjs";

export function commandDescriptor<
  Kind extends Contract.WorkspaceCommand["command"],
>(
  kind: Kind,
): Extract<(typeof Contract.COMMAND_DESCRIPTORS)[number], { kind: Kind }> {
  const descriptor = ContractCommands.find(
    (
      entry,
    ): entry is Extract<(typeof ContractCommands)[number], { kind: Kind }> =>
      entry.kind === kind,
  );
  if (!descriptor) throw new Error(`Missing command descriptor: ${kind}`);
  return descriptor;
}
export type JsonValue =
  null | boolean | number | string | JsonValue[] | { [key: string]: JsonValue };

export interface JsonRpcMessageEnvelope {
  jsonrpc: "2.0";
  id?: Contract.RpcId;
  method: string;
  params?: object | JsonValue[];
}

export interface JsonRpcResponseEnvelope {
  jsonrpc: "2.0";
  id: Contract.RpcId | null;
  result?: JsonValue;
  error?: { code: number; message: string; data?: JsonValue };
}

export type LspMethod =
  | "initialize"
  | "initialized"
  | "shutdown"
  | "exit"
  | `textDocument/${string}`
  | `workspace/${string}`
  | `window/${string}`
  | `$/${string}`;

export interface PublishDiagnosticsParams {
  uri: string;
  version?: number;
  diagnostics: Diagnostic[];
}

export interface Diagnostic {
  range: Contract.TextRange;
  severity?: number;
  code?: string | number;
  codeDescription?: { href: string };
  source?: string;
  message: string;
  tags?: number[];
  relatedInformation?: {
    location: { uri: string; range: Contract.TextRange };
    message: string;
  }[];
  data?: JsonValue;
}

export interface ServerNotificationMap
  extends Contract.FleximarkServerNotificationMap {
  "textDocument/publishDiagnostics": PublishDiagnosticsParams;
}

export type Validator<T> = (value: unknown) => value is T;
type ContractTypeName = keyof Contract.WireTypeMap;
type ValidatorEntry = (typeof TYPE_VALIDATORS)[ContractTypeName];

const validateWire = (entry: ValidatorEntry, value: unknown): boolean =>
  validateContractSchema(
    CONTRACT_DEFINITIONS,
    { $ref: `#/$defs/${entry.schema}` },
    value,
  ) &&
  (entry.semantic === null || semanticHandlers[entry.semantic](value));

const wireValidator =
  <Entry extends ValidatorEntry>(
    entry: Entry,
  ): Validator<Contract.WireTypeMap[Entry["schema"]]> =>
  (value): value is Contract.WireTypeMap[Entry["schema"]] =>
    validateWire(entry, value);

export function hasContractShape<Name extends ContractTypeName>(
  name: Name,
  value: unknown,
): value is Contract.WireTypeMap[Name] {
  return validateWire(TYPE_VALIDATORS[name], value);
}

const contractValidator =
  <Name extends ContractTypeName>(
    name: Name,
  ): Validator<Contract.WireTypeMap[Name]> =>
  (value): value is Contract.WireTypeMap[Name] =>
    hasContractShape(name, value);

export const isTextRange = contractValidator("range");
export const isRenderFrame = contractValidator("renderFrame");
export const isPreviewNavigationEvent = contractValidator(
  "previewNavigationEvent",
);
export const isRenderNavigationEvent = contractValidator(
  "renderNavigationEvent",
);

export function shouldForwardPreviewNavigation(
  value: unknown,
  previewSessionId: string,
  renderRevision: number,
): value is Contract.PreviewNavigationEvent {
  return (
    isPreviewNavigationEvent(value) &&
    value.previewSessionId === previewSessionId &&
    value.renderRevision === renderRevision
  );
}

export type PreviewHostEvent =
  Contract.RenderFrame | Contract.RenderNavigationEvent;
export const isPreviewHostEvent: Validator<PreviewHostEvent> = (
  value,
): value is PreviewHostEvent =>
  isRenderFrame(value) || isRenderNavigationEvent(value);

const string = (value: unknown): value is string => typeof value === "string";
const safeInteger = (value: unknown): value is number =>
  Number.isSafeInteger(value);
const array =
  <T,>(validator: Validator<T>): Validator<T[]> =>
  (value): value is T[] =>
    Array.isArray(value) && value.every((item) => validator(item));

function shape(
  value: unknown,
  required: Record<string, Validator<unknown>>,
  optionalFields: Record<string, Validator<unknown>> = {},
): boolean {
  if (!isRecord(value)) return false;
  const allowed = new Set([
    ...Object.keys(required),
    ...Object.keys(optionalFields),
  ]);
  return (
    Object.keys(value).every((key) => allowed.has(key)) &&
    Object.entries(required).every(
      ([key, validator]) => Object.hasOwn(value, key) && validator(value[key]),
    ) &&
    Object.entries(optionalFields).every(
      ([key, validator]) => !Object.hasOwn(value, key) || validator(value[key]),
    )
  );
}

type RequestParamsValidators = {
  [Method in keyof Contract.CustomRequestMap]: Validator<
    Contract.CustomRequestMap[Method]["params"]
  >;
};
type RequestResultValidators = {
  [Method in keyof Contract.CustomRequestMap]: Validator<
    Contract.CustomRequestMap[Method]["result"]
  >;
};
type ClientValidators = {
  [Method in keyof Contract.ClientNotificationMap]: Validator<
    Contract.ClientNotificationMap[Method]
  >;
};

const wireValidatorMap = <Entry,>(
  entries: Readonly<Record<string, Entry>>,
  select: (entry: Entry) => ValidatorEntry,
): Record<string, Validator<unknown>> =>
  Object.fromEntries(
    Object.entries(entries).map(([method, entry]) => [
      method,
      wireValidator(select(entry)),
    ]),
  );

export const customRequestParamsValidators = wireValidatorMap(
  REQUEST_VALIDATORS,
  ([paramsIndex]) => paramsIndex,
) as RequestParamsValidators;

export const customRequestResultValidators = wireValidatorMap(
  REQUEST_VALIDATORS,
  ([, resultIndex]) => resultIndex,
) as RequestResultValidators;

export const clientNotificationValidators = wireValidatorMap(
  CLIENT_NOTIFICATION_VALIDATORS,
  (index) => index,
) as ClientValidators;

const fleximarkServerNotificationValidators = wireValidatorMap(
  SERVER_NOTIFICATION_VALIDATORS,
  (index) => index,
) as {
  [Method in keyof Contract.FleximarkServerNotificationMap]: Validator<
    Contract.FleximarkServerNotificationMap[Method]
  >;
};

export const isJsonValue: Validator<JsonValue> = (value): value is JsonValue =>
  value === null ||
  typeof value === "boolean" ||
  typeof value === "string" ||
  (typeof value === "number" && Number.isFinite(value)) ||
  (Array.isArray(value) && value.every(isJsonValue)) ||
  (isRecord(value) && Object.values(value).every(isJsonValue));

const isJsonRpcId = (value: unknown): value is Contract.RpcId =>
  hasContractShape("id", value);

export function isJsonRpcMessageEnvelope(
  value: unknown,
): value is JsonRpcMessageEnvelope {
  return shape(
    value,
    { jsonrpc: (item) => item === "2.0", method: string },
    {
      id: isJsonRpcId,
      params: (item) => isRecord(item) || array(isJsonValue)(item),
    },
  );
}

export function isJsonRpcResponseEnvelope(
  value: unknown,
): value is JsonRpcResponseEnvelope {
  if (!isRecord(value)) return false;
  const hasResult = Object.hasOwn(value, "result");
  const hasError = Object.hasOwn(value, "error");
  return (
    hasResult !== hasError &&
    shape(
      value,
      {
        jsonrpc: (item) => item === "2.0",
        id: (item) => item === null || isJsonRpcId(item),
      },
      {
        result: isJsonValue,
        error: (item): item is NonNullable<JsonRpcResponseEnvelope["error"]> =>
          shape(
            item,
            { code: safeInteger, message: string },
            { data: isJsonValue },
          ),
      },
    )
  );
}

const isDiagnostic: Validator<Diagnostic> = (value): value is Diagnostic =>
  shape(
    value,
    { range: isTextRange, message: string },
    {
      severity: safeInteger,
      code: (item) => string(item) || safeInteger(item),
      codeDescription: (item): item is { href: string } =>
        shape(item, { href: string }),
      source: string,
      tags: array(safeInteger),
      relatedInformation: array(
        (item): item is NonNullable<Diagnostic["relatedInformation"]>[number] =>
          shape(item, {
            location: (
              location,
            ): location is NonNullable<
              Diagnostic["relatedInformation"]
            >[number]["location"] =>
              shape(location, { uri: string, range: isTextRange }),
            message: string,
          }),
      ),
      data: isJsonValue,
    },
  );

export const serverNotificationValidators: {
  [Method in keyof ServerNotificationMap]: Validator<
    ServerNotificationMap[Method]
  >;
} = {
  ...fleximarkServerNotificationValidators,
  "textDocument/publishDiagnostics": (
    value,
  ): value is PublishDiagnosticsParams =>
    shape(
      value,
      { uri: string, diagnostics: array(isDiagnostic) },
      { version: safeInteger },
    ),
};

export function isCustomRequestMethod(
  method: string,
): method is keyof Contract.CustomRequestMap {
  return Object.hasOwn(customRequestParamsValidators, method);
}

export function isServerNotificationMethod(
  method: string,
): method is keyof ServerNotificationMap {
  return Object.hasOwn(serverNotificationValidators, method);
}
