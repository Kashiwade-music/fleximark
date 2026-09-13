/** Browser-neutral consumers of the Rust-owned FlexiMark wire contract. */
import { validateContractDescriptor } from "./contract-validator.mjs";
import { isSafePatchAttribute } from "./patch-attributes.mjs";
import {
  CLIENT_NOTIFICATION_VALIDATORS,
  PROTOCOL_VERSION,
  REQUEST_VALIDATORS,
  SERVER_NOTIFICATION_VALIDATORS,
  TYPE_VALIDATORS,
  VALIDATOR_TYPES,
} from "./protocol.generated.mjs";
import type * as Contract from "./protocol.generated.mjs";

export { PROTOCOL_VERSION };
export type {
  AttachDocumentResult,
  CheckpointDocumentResult,
  CommandResult,
  CreatePreviewResult,
  ExecuteCommandParams,
  GetNoteOptionsParams,
  GetNoteOptionsResult,
  InitializeParams,
  InitializeResult,
  NavigationEntry,
  PatchOperation,
  PreviewTarget,
  RenderAsset,
  RenderPatch,
  RenderPublication,
  RenderSnapshot,
  RenderStyle,
  RequestFullTextParams,
  SourceNavigationEvent,
  SourcePosition,
  SourceRange,
} from "./protocol.generated.mjs";

export type Position = Contract.TextPosition;
export type TextRange = Contract.TextRange;
export type TextSelection = Contract.TextSelection;
export type EditorNavigationEvent = Contract.PreviewNavigationEvent;
export type PreviewNavigationEvent = Contract.RenderNavigationEvent;
export type ClientPreviewEventParams = Contract.PreviewEventParams;
export type ServerPreviewEventParams = Contract.ServerPreviewEventParams;
export type RpcId = Contract.RpcId;
export type CustomRequestMap = Contract.CustomRequestMap;
export type ClientNotificationMap = Contract.ClientNotificationMap;

/** Historical public aliases retained at the adapter boundary. */
export type PreviewEvent = ServerPreviewEventParams;
export type PreviewClientEvent = EditorNavigationEvent;

export type JsonValue =
  null | boolean | number | string | JsonValue[] | { [key: string]: JsonValue };

export interface JsonRpcMessageEnvelope {
  jsonrpc: "2.0";
  id?: RpcId;
  method: string;
  params?: object | JsonValue[];
}

export interface JsonRpcResponseEnvelope {
  jsonrpc: "2.0";
  id: RpcId | null;
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
  range: TextRange;
  severity?: number;
  code?: string | number;
  codeDescription?: { href: string };
  source?: string;
  message: string;
  tags?: number[];
  relatedInformation?: {
    location: { uri: string; range: TextRange };
    message: string;
  }[];
  data?: JsonValue;
}

export interface ServerNotificationMap
  extends Contract.FleximarkServerNotificationMap {
  "textDocument/publishDiagnostics": PublishDiagnosticsParams;
}

/** This envelope belongs to the browser host, not to the daemon protocol. */
export type WebviewInboundMessage =
  | { type: "ready" }
  | { type: "requestSnapshot" }
  | { type: "rendered"; previewSessionId: string; renderRevision: number }
  | EditorNavigationEvent;

export type Validator<T> = ((value: unknown) => boolean) & {
  readonly __validatedType?: T;
};

const object = (value: unknown): value is Record<string, unknown> =>
  value !== null && typeof value === "object" && !Array.isArray(value);
const string = (value: unknown): value is string => typeof value === "string";
const safeInteger = (value: unknown): value is number =>
  Number.isSafeInteger(value);
const array =
  <T,>(validator: Validator<T>): Validator<T[]> =>
  (value) =>
    Array.isArray(value) && value.every((item) => validator(item));

function shape(
  value: unknown,
  required: Record<string, Validator<unknown>>,
  optionalFields: Record<string, Validator<unknown>> = {},
): boolean {
  if (!object(value)) return false;
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

type ContractTypeName = keyof typeof TYPE_VALIDATORS;

const hasContractShape = (name: ContractTypeName, value: unknown): boolean =>
  validateContractDescriptor(VALIDATOR_TYPES, TYPE_VALIDATORS[name], value);

const decodedBase64Length = (value: string): number | undefined => {
  if (
    value.length % 4 !== 0 ||
    !/^(?:[A-Za-z0-9+/]{4})*(?:[A-Za-z0-9+/]{2}==|[A-Za-z0-9+/]{3}=)?$/.test(
      value,
    )
  )
    return;
  return (
    (value.length / 4) * 3 -
    (value.endsWith("==") ? 2 : value.endsWith("=") ? 1 : 0)
  );
};

const rangeOrderIsValid = (range: Contract.SourceRange): boolean =>
  range.byteEnd >= range.byteStart &&
  (range.start.line < range.end.line ||
    (range.start.line === range.end.line &&
      range.start.character <= range.end.character));

const navigationIsValid = (
  navigation: readonly Contract.NavigationEntry[],
): boolean =>
  navigation.every(({ sourceRange }) => rangeOrderIsValid(sourceRange));

const assetIsValid = (asset: Contract.RenderAsset): boolean =>
  asset.reference === `fleximark-asset:${asset.contentHash}` &&
  decodedBase64Length(asset.data) === asset.byteLength;

const assetsAreValid = (assets: readonly Contract.RenderAsset[]): boolean =>
  assets.every(assetIsValid) &&
  new Set(assets.map(({ reference }) => reference)).size === assets.length &&
  assets.reduce((total, { byteLength }) => total + byteLength, 0) <=
    8 * 1024 * 1024;

const patchOperationsAreValid = (
  operations: readonly Contract.PatchOperation[],
): boolean =>
  operations.every(
    (operation) =>
      operation.type !== "setAttributes" ||
      Object.entries(operation.attributes).every(([name, value]) =>
        isSafePatchAttribute(name, value),
      ),
  );

const publicationSemanticsAreValid = (
  publication: Contract.RenderPublication,
): boolean => {
  if (!navigationIsValid(publication.navigation)) return false;
  if (publication.type === "full") return assetsAreValid(publication.assets);
  return patchOperationsAreValid(publication.operations);
};

const sourceNavigationSemanticsAreValid = (
  event: Contract.SourceNavigationEvent,
): boolean => rangeOrderIsValid(event.sourceRange);

const previewEventParamsSemanticsAreValid = (
  params: Contract.PreviewEventParams,
): boolean =>
  params.event.previewSessionId === params.previewSessionId &&
  params.event.renderRevision === params.renderRevision;

const serverPreviewEventParamsSemanticsAreValid = (
  params: Contract.ServerPreviewEventParams,
): boolean => {
  const event = params.event;
  if (event.type === "selectSource" || event.type === "revealSource")
    return sourceNavigationSemanticsAreValid(event);
  if (event.type === "selection" || event.type === "viewport")
    return (
      event.previewSessionId === params.previewSessionId &&
      event.renderRevision === params.renderRevision
    );
  return (
    publicationSemanticsAreValid(event) &&
    event.previewSessionId === params.previewSessionId &&
    event.resultRenderRevision === params.renderRevision
  );
};

function semanticContractCheck(index: number, value: unknown): boolean {
  if (index === TYPE_VALIDATORS.renderPublication)
    return publicationSemanticsAreValid(value as Contract.RenderPublication);
  if (index === TYPE_VALIDATORS.createPreviewResult) {
    const result = value as Contract.CreatePreviewResult;
    return (
      publicationSemanticsAreValid(result.initialPublication) &&
      result.initialPublication.previewSessionId === result.previewSessionId
    );
  }
  if (index === TYPE_VALIDATORS.previewEventParams)
    return previewEventParamsSemanticsAreValid(
      value as Contract.PreviewEventParams,
    );
  if (index === TYPE_VALIDATORS.serverPreviewEventParams)
    return serverPreviewEventParamsSemanticsAreValid(
      value as Contract.ServerPreviewEventParams,
    );
  return true;
}

const wireValidator =
  <T,>(descriptorIndex: number): Validator<T> =>
  (value): boolean => {
    return (
      validateContractDescriptor(VALIDATOR_TYPES, descriptorIndex, value) &&
      semanticContractCheck(descriptorIndex, value)
    );
  };

export const isPosition: Validator<Position> = (value): value is Position =>
  hasContractShape("position", value);

export const isTextRange: Validator<TextRange> = (value): value is TextRange =>
  hasContractShape("range", value);

export const isSourcePosition: Validator<Contract.SourcePosition> = (
  value,
): value is Contract.SourcePosition =>
  hasContractShape("sourcePosition", value);

export const isSourceRange: Validator<Contract.SourceRange> = (
  value,
): value is Contract.SourceRange =>
  hasContractShape("sourceRange", value) &&
  rangeOrderIsValid(value as Contract.SourceRange);

export const isRenderSnapshot: Validator<Contract.RenderSnapshot> = (
  value,
): value is Contract.RenderSnapshot =>
  hasContractShape("renderSnapshot", value) &&
  publicationSemanticsAreValid(value as Contract.RenderSnapshot);

export const isRenderPatch: Validator<Contract.RenderPatch> = (
  value,
): value is Contract.RenderPatch =>
  hasContractShape("renderPatch", value) &&
  publicationSemanticsAreValid(value as Contract.RenderPatch);

export const isRenderPublication: Validator<Contract.RenderPublication> = (
  value,
): value is Contract.RenderPublication =>
  hasContractShape("renderPublication", value) &&
  publicationSemanticsAreValid(value as Contract.RenderPublication);

export const isEditorNavigationEvent: Validator<EditorNavigationEvent> = (
  value,
): value is EditorNavigationEvent =>
  hasContractShape("previewNavigationEvent", value);

export const isPreviewNavigationEvent: Validator<PreviewNavigationEvent> = (
  value,
): value is PreviewNavigationEvent =>
  hasContractShape("renderNavigationEvent", value);

export const isSourceNavigationEvent: Validator<
  Contract.SourceNavigationEvent
> = (value): value is Contract.SourceNavigationEvent =>
  hasContractShape("sourceNavigationEvent", value) &&
  sourceNavigationSemanticsAreValid(value as Contract.SourceNavigationEvent);

export function shouldForwardEditorNavigation(
  value: unknown,
  previewSessionId: string,
  renderRevision: number,
): value is EditorNavigationEvent {
  return (
    isEditorNavigationEvent(value) &&
    (value as EditorNavigationEvent).previewSessionId === previewSessionId &&
    (value as EditorNavigationEvent).renderRevision === renderRevision
  );
}

export function isWebviewInboundMessage(
  value: unknown,
): value is WebviewInboundMessage {
  if (!object(value)) return false;
  if (value.type === "ready" || value.type === "requestSnapshot")
    return Object.keys(value).length === 1;
  if (value.type === "rendered")
    return shape(value, {
      type: (item) => item === "rendered",
      previewSessionId: (item) => string(item) && item.length > 0,
      renderRevision: (item) => safeInteger(item) && item >= 1,
    });
  return isEditorNavigationEvent(value);
}

export type PreviewHostEvent =
  Contract.RenderPublication | PreviewNavigationEvent;
export const isPreviewHostEvent: Validator<PreviewHostEvent> = (
  value,
): value is PreviewHostEvent =>
  isRenderPublication(value) || isPreviewNavigationEvent(value);

type RequestParamsValidators = {
  [Method in keyof CustomRequestMap]: Validator<
    CustomRequestMap[Method]["params"]
  >;
};
type RequestResultValidators = {
  [Method in keyof CustomRequestMap]: Validator<
    CustomRequestMap[Method]["result"]
  >;
};
type ClientValidators = {
  [Method in keyof ClientNotificationMap]: Validator<
    ClientNotificationMap[Method]
  >;
};

export const customRequestParamsValidators = Object.fromEntries(
  Object.entries(REQUEST_VALIDATORS).map(([method, [paramsIndex]]) => [
    method,
    wireValidator(paramsIndex),
  ]),
) as RequestParamsValidators;

export const customRequestResultValidators = Object.fromEntries(
  Object.entries(REQUEST_VALIDATORS).map(([method, [, resultIndex]]) => [
    method,
    wireValidator(resultIndex),
  ]),
) as RequestResultValidators;

export const clientNotificationValidators = Object.fromEntries(
  Object.entries(CLIENT_NOTIFICATION_VALIDATORS).map(([method, index]) => [
    method,
    wireValidator(index),
  ]),
) as ClientValidators;

const fleximarkServerNotificationValidators = Object.fromEntries(
  Object.entries(SERVER_NOTIFICATION_VALIDATORS).map(([method, index]) => [
    method,
    wireValidator(index),
  ]),
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
  (object(value) && Object.values(value).every(isJsonValue));

const isJsonRpcId = (value: unknown): boolean => hasContractShape("id", value);

export function isJsonRpcMessageEnvelope(
  value: unknown,
): value is JsonRpcMessageEnvelope {
  return shape(
    value,
    { jsonrpc: (item) => item === "2.0", method: string },
    {
      id: isJsonRpcId,
      params: (item) => object(item) || array(isJsonValue)(item),
    },
  );
}

export function isJsonRpcResponseEnvelope(
  value: unknown,
): value is JsonRpcResponseEnvelope {
  if (!object(value)) return false;
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
        error: (item) =>
          shape(
            item,
            { code: safeInteger, message: string },
            { data: isJsonValue },
          ),
      },
    )
  );
}

const isDiagnostic: Validator<Diagnostic> = (value) =>
  shape(
    value,
    { range: isTextRange, message: string },
    {
      severity: safeInteger,
      code: (item) => string(item) || safeInteger(item),
      codeDescription: (item) => shape(item, { href: string }),
      source: string,
      tags: array(safeInteger),
      relatedInformation: array((item) =>
        shape(item, {
          location: (location) =>
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
  "textDocument/publishDiagnostics": (value) =>
    shape(
      value,
      { uri: string, diagnostics: array(isDiagnostic) },
      { version: safeInteger },
    ),
};

export function isCustomRequestMethod(
  method: string,
): method is keyof CustomRequestMap {
  return Object.hasOwn(customRequestParamsValidators, method);
}

export function isServerNotificationMethod(
  method: string,
): method is keyof ServerNotificationMap {
  return Object.hasOwn(serverNotificationValidators, method);
}
