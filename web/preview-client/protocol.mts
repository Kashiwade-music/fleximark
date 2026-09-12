/** Browser-neutral FlexiMark protocol types and runtime validators. */
import { isSafePatchAttribute } from "./patch-attributes.mjs";

export type JsonValue =
  null | boolean | number | string | JsonValue[] | { [key: string]: JsonValue };

export interface JsonRpcMessageEnvelope {
  jsonrpc: "2.0";
  id?: number | string;
  method: string;
  params?: object | JsonValue[];
}

export interface JsonRpcResponseEnvelope {
  jsonrpc: "2.0";
  id: number | string;
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

export interface Position {
  line: number;
  character: number;
}

export interface TextRange {
  start: Position;
  end: Position;
}

export interface TextSelection {
  anchor: Position;
  active: Position;
}

export interface SourcePosition extends Position {
  encoding: "utf8" | "utf16" | "utf32";
}

export interface SourceRange {
  byteStart: number;
  byteEnd: number;
  start: SourcePosition;
  end: SourcePosition;
}

export interface NavigationEntry {
  nodeId: string;
  sourceRange: SourceRange;
  depth: number;
}

export interface RenderStyle {
  css: string;
  fingerprint: string;
}

export interface RenderAsset {
  reference: string;
  mediaType: string;
  contentHash: string;
  byteLength: number;
  data: string;
}

interface Precondition {
  nodeExists: true;
  currentParentId: string;
}

export type PatchOperation =
  | {
      type: "insert";
      nodeId: string;
      parentId: string;
      beforeId: string | null;
      afterId: string | null;
      atEnd: boolean;
      contentNodeIds: string[];
      content: string;
    }
  | {
      type: "remove";
      nodeId: string;
      parentId: string;
      precondition: Precondition;
    }
  | {
      type: "replace";
      nodeId: string;
      parentId: string;
      contentNodeIds: string[];
      content: string;
      precondition: Precondition;
    }
  | {
      type: "move";
      nodeId: string;
      parentId: string;
      beforeId: string | null;
      afterId: string | null;
      atEnd: boolean;
      precondition: Precondition;
    }
  | {
      type: "setAttributes";
      nodeId: string;
      parentId: string;
      attributes: Record<string, string | null>;
      precondition: Precondition;
    };

export interface RenderSnapshot {
  type: "full";
  previewSessionId: string;
  documentVersion: number;
  resultRenderRevision: number;
  rendererFingerprint: string;
  nodeIds: string[];
  navigation: NavigationEntry[];
  style: RenderStyle | null;
  assets: RenderAsset[];
  html: string;
}

export interface RenderPatch {
  type: "patch";
  previewSessionId: string;
  documentVersion: number;
  baseRenderRevision: number;
  resultRenderRevision: number;
  baseRendererFingerprint: string;
  resultRendererFingerprint: string;
  navigation: NavigationEntry[];
  style: RenderStyle | null;
  operations: PatchOperation[];
}

export type RenderPublication = RenderSnapshot | RenderPatch;

export type PreviewNavigationEvent =
  | {
      type: "selection";
      previewSessionId: string;
      renderRevision: number;
      nodeIds: string[];
      activePosition?: Position | null;
    }
  | {
      type: "viewport";
      previewSessionId: string;
      renderRevision: number;
      nodeId: string;
    };

export interface EditorNavigationEvent {
  type: "selectNode" | "revealNode";
  previewSessionId: string;
  renderRevision: number;
  nodeId: string;
}

export type SourceNavigationEvent =
  | { type: "selectSource"; sourceRange: SourceRange }
  | { type: "revealSource"; sourceRange: SourceRange };

export interface InitializeParams {
  protocolVersion: 1;
  client: { name: string; version: string };
  capabilities?: {
    embeddedHtml?: boolean;
    structuredPreview?: boolean;
    selectionEvents?: boolean;
    viewportEvents?: boolean;
    openExternal?: boolean;
  };
  workspaces?: { uri: string; trusted: boolean }[];
}

export interface InitializeResult {
  protocolVersion: 1;
  daemonInstanceId: string;
  workspaceStatuses: { uri: string; enabled: boolean; error?: string }[];
  capabilities: {
    htmlRender: boolean;
    documentCheckpoint: boolean;
    selectionEvents: boolean;
    viewportEvents: boolean;
    workspaceCommands: string[];
  };
}

export interface AttachDocumentResult {
  documentSessionId: string;
  documentVersion: number;
  contentHash: string;
}

export interface CheckpointDocumentResult {
  documentVersion: number;
  contentHash: string;
}

export type PreviewTarget = "embeddedHtml" | "externalBrowser";

export interface CreatePreviewResult {
  previewSessionId: string;
  url?: string;
  initialPublication: RenderSnapshot;
}

export interface CommandResult {
  message?: { level: "info" | "warning" | "error"; text: string };
  openUri?: string;
}

export interface ExecuteCommandParams {
  daemonInstanceId: string;
  command: string;
  documentSessionId?: string;
  expectedDocumentVersion?: number;
  workspaceUri?: string;
  destinationUri?: string;
  noteCategory?: string;
  noteTemplate?: string;
}

export interface GetNoteOptionsParams {
  daemonInstanceId: string;
  workspaceUri: string;
}

export interface GetNoteOptionsResult {
  categories: string[];
  templates: string[];
}

export interface RequestFullTextParams {
  daemonInstanceId: string;
  uri: string;
  documentSessionId: string;
  reason: string;
}

export interface ClientPreviewEventParams {
  daemonInstanceId: string;
  previewSessionId: string;
  renderRevision: number;
  event: EditorNavigationEvent;
}

export interface ServerPreviewEventParams {
  daemonInstanceId: string;
  previewSessionId: string;
  renderRevision: number;
  event: RenderPublication | PreviewNavigationEvent | SourceNavigationEvent;
}

/** Kept as the historical public name for daemon-to-client preview events. */
export type PreviewEvent = ServerPreviewEventParams;
export type PreviewClientEvent = EditorNavigationEvent;

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

export type WebviewInboundMessage =
  | { type: "ready" }
  | { type: "requestSnapshot" }
  | { type: "rendered"; previewSessionId: string; renderRevision: number }
  | EditorNavigationEvent;

export interface CustomRequestMap {
  "fleximark/initialize": {
    params: InitializeParams;
    result: InitializeResult;
  };
  "fleximark/attachDocument": {
    params: {
      daemonInstanceId: string;
      uri: string;
      expectedDocumentVersion: number;
      contentHash: string;
    };
    result: AttachDocumentResult;
  };
  "fleximark/checkpointDocument": {
    params: {
      daemonInstanceId: string;
      documentSessionId: string;
      documentVersion: number;
      contentHash: string;
    };
    result: CheckpointDocumentResult;
  };
  "fleximark/render": {
    params: {
      daemonInstanceId: string;
      documentSessionId: string;
      documentVersion: number;
    };
    result: RenderPublication;
  };
  "fleximark/createPreview": {
    params: {
      daemonInstanceId: string;
      documentSessionId: string;
      expectedDocumentVersion: number;
      target: PreviewTarget;
    };
    result: CreatePreviewResult;
  };
  "fleximark/disposePreview": {
    params: { daemonInstanceId: string; previewSessionId: string };
    result: null;
  };
  "fleximark/reloadPreview": {
    params: { daemonInstanceId: string; previewSessionId: string };
    result: null;
  };
  "fleximark/getNoteOptions": {
    params: GetNoteOptionsParams;
    result: GetNoteOptionsResult;
  };
  "fleximark/reconfigureWorkspace": {
    params: {
      daemonInstanceId: string;
      workspaceUri: string;
      trusted: boolean;
    };
    result: null;
  };
  "fleximark/executeCommand": {
    params: ExecuteCommandParams;
    result: CommandResult;
  };
  "fleximark/openDocument": {
    params: {
      daemonInstanceId: string;
      uri: string;
      documentVersion: number;
      text: string;
    };
    result: AttachDocumentResult;
  };
  "fleximark/changeDocument": {
    params: {
      daemonInstanceId: string;
      documentSessionId: string;
      baseDocumentVersion: number;
      baseContentHash: string;
      documentVersion: number;
      text: string;
    };
    result: CheckpointDocumentResult;
  };
  "fleximark/closeDocument": {
    params: { daemonInstanceId: string; documentSessionId: string };
    result: null;
  };
}

export interface ClientNotificationMap {
  "fleximark/setSelection": {
    daemonInstanceId: string;
    documentSessionId: string;
    expectedDocumentVersion: number;
    selections: TextSelection[];
  };
  "fleximark/setViewport": {
    daemonInstanceId: string;
    documentSessionId: string;
    expectedDocumentVersion: number;
    ranges: TextRange[];
  };
  "fleximark/previewEvent": ClientPreviewEventParams;
}

export interface ServerNotificationMap {
  "fleximark/requestFullText": RequestFullTextParams;
  "fleximark/previewEvent": ServerPreviewEventParams;
  "textDocument/publishDiagnostics": PublishDiagnosticsParams;
}

export type Validator<T> = ((value: unknown) => boolean) & {
  readonly __validatedType?: T;
};

const object = (value: unknown): value is Record<string, unknown> =>
  value !== null && typeof value === "object" && !Array.isArray(value);
const safeInteger = (value: unknown, minimum?: number): value is number =>
  Number.isSafeInteger(value) &&
  (minimum === undefined || (value as number) >= minimum);
const string = (value: unknown): value is string => typeof value === "string";
const nonEmptyString = (value: unknown): value is string =>
  string(value) && value.length > 0;
const boolean = (value: unknown): value is boolean =>
  typeof value === "boolean";
const nullable =
  <T,>(validator: Validator<T>): Validator<T | null> =>
  (value) =>
    value === null || validator(value);
const array =
  <T,>(validator: Validator<T>): Validator<T[]> =>
  (value) =>
    Array.isArray(value) && value.every((item) => validator(item));
const uniqueStrings: Validator<string[]> = (value): value is string[] =>
  Array.isArray(value) &&
  value.every(string) &&
  new Set(value).size === value.length;
const uniqueNonEmptyStrings: Validator<string[]> = (value): value is string[] =>
  Array.isArray(value) &&
  value.every(nonEmptyString) &&
  new Set(value).size === value.length;

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
  if (Object.keys(value).some((key) => !allowed.has(key))) return false;
  return (
    Object.entries(required).every(
      ([key, validator]) => Object.hasOwn(value, key) && validator(value[key]),
    ) &&
    Object.entries(optionalFields).every(
      ([key, validator]) => !Object.hasOwn(value, key) || validator(value[key]),
    )
  );
}

export const isPosition: Validator<Position> = (value): value is Position =>
  shape(value, {
    line: (item) => safeInteger(item, 0),
    character: (item) => safeInteger(item, 0),
  });

export const isTextRange: Validator<TextRange> = (value): value is TextRange =>
  shape(value, { start: isPosition, end: isPosition });

const isTextSelection: Validator<TextSelection> = (value) =>
  shape(value, { anchor: isPosition, active: isPosition });

export const isSourcePosition: Validator<SourcePosition> = (
  value,
): value is SourcePosition =>
  shape(value, {
    line: (item) => safeInteger(item, 0),
    character: (item) => safeInteger(item, 0),
    encoding: (item) => ["utf8", "utf16", "utf32"].includes(item as string),
  });

export const isSourceRange: Validator<SourceRange> = (
  value,
): value is SourceRange => {
  if (
    !object(value) ||
    !shape(value, {
      byteStart: (item) => safeInteger(item, 0),
      byteEnd: (item) => safeInteger(item, 0),
      start: isSourcePosition,
      end: isSourcePosition,
    })
  )
    return false;
  const range = value as unknown as SourceRange;
  return (
    range.byteEnd >= range.byteStart &&
    (range.start.line < range.end.line ||
      (range.start.line === range.end.line &&
        range.start.character <= range.end.character))
  );
};

const isNavigationEntry: Validator<NavigationEntry> = (value) =>
  shape(value, {
    nodeId: nonEmptyString,
    sourceRange: isSourceRange,
    depth: (item) => safeInteger(item, 0),
  });

const isRenderStyle: Validator<RenderStyle> = (value) =>
  shape(value, {
    css: string,
    fingerprint: (item) => string(item) && /^[0-9a-f]{64}$/.test(item),
  });

const mediaTypes = new Set([
  "image/png",
  "image/jpeg",
  "image/gif",
  "image/webp",
  "audio/mpeg",
  "audio/ogg",
  "audio/wav",
]);
function decodedBase64Length(value: string): number | undefined {
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
}

const isRenderAsset: Validator<RenderAsset> = (value) => {
  if (
    !object(value) ||
    !shape(value, {
      reference: (item) =>
        string(item) && /^fleximark-asset:[0-9a-f]{64}$/.test(item),
      mediaType: (item) => string(item) && mediaTypes.has(item),
      contentHash: (item) => string(item) && /^[0-9a-f]{64}$/.test(item),
      byteLength: (item) => safeInteger(item, 0) && item <= 1024 * 1024,
      data: string,
    })
  )
    return false;
  const asset = value as unknown as RenderAsset;
  return (
    asset.reference === `fleximark-asset:${asset.contentHash}` &&
    decodedBase64Length(asset.data) === asset.byteLength
  );
};

const isRenderAssets: Validator<RenderAsset[]> = (value) => {
  if (!Array.isArray(value) || !value.every(isRenderAsset)) return false;
  const assets = value as RenderAsset[];
  return (
    new Set(assets.map(({ reference }) => reference)).size === assets.length &&
    assets.reduce((total, { byteLength }) => total + byteLength, 0) <=
      8 * 1024 * 1024
  );
};

const isPrecondition: Validator<Precondition> = (value) =>
  shape(value, {
    nodeExists: (item) => item === true,
    currentParentId: nonEmptyString,
  });

const anchorFields = {
  beforeId: (item: unknown) => item === null || string(item),
  afterId: (item: unknown) => item === null || string(item),
  atEnd: boolean,
};
const commonOperationFields = { nodeId: string, parentId: string };
const isPatchOperation: Validator<PatchOperation> = (value) => {
  if (!object(value)) return false;
  switch (value.type) {
    case "insert":
      return shape(value, {
        type: (item) => item === "insert",
        ...commonOperationFields,
        ...anchorFields,
        contentNodeIds: uniqueStrings,
        content: string,
      });
    case "remove":
      return shape(value, {
        type: (item) => item === "remove",
        ...commonOperationFields,
        precondition: isPrecondition,
      });
    case "replace":
      return shape(value, {
        type: (item) => item === "replace",
        ...commonOperationFields,
        contentNodeIds: uniqueStrings,
        content: string,
        precondition: isPrecondition,
      });
    case "move":
      return shape(value, {
        type: (item) => item === "move",
        ...commonOperationFields,
        ...anchorFields,
        precondition: isPrecondition,
      });
    case "setAttributes":
      return (
        shape(value, {
          type: (item) => item === "setAttributes",
          ...commonOperationFields,
          attributes: object,
          precondition: isPrecondition,
        }) &&
        Object.entries(value.attributes as Record<string, unknown>).every(
          ([name, item]) => isSafePatchAttribute(name, item),
        )
      );
    default:
      return false;
  }
};

export const isRenderSnapshot: Validator<RenderSnapshot> = (
  value,
): value is RenderSnapshot => {
  if (
    !object(value) ||
    !shape(value, {
      type: (item) => item === "full",
      previewSessionId: string,
      documentVersion: (item) => safeInteger(item, 0),
      resultRenderRevision: (item) => safeInteger(item, 1),
      rendererFingerprint: string,
      nodeIds: uniqueNonEmptyStrings,
      navigation: array(isNavigationEntry),
      style: nullable(isRenderStyle),
      assets: isRenderAssets,
      html: string,
    })
  )
    return false;
  return (value.nodeIds as unknown[]).length >= 1;
};

export const isRenderPatch: Validator<RenderPatch> = (
  value,
): value is RenderPatch =>
  shape(value, {
    type: (item) => item === "patch",
    previewSessionId: string,
    documentVersion: (item) => safeInteger(item, 0),
    baseRenderRevision: (item) => safeInteger(item, 1),
    resultRenderRevision: (item) => safeInteger(item, 1),
    baseRendererFingerprint: string,
    resultRendererFingerprint: string,
    navigation: array(isNavigationEntry),
    style: nullable(isRenderStyle),
    operations: array(isPatchOperation),
  });

export const isRenderPublication: Validator<RenderPublication> = (
  value,
): value is RenderPublication =>
  isRenderSnapshot(value) || isRenderPatch(value);

export const isEditorNavigationEvent: Validator<EditorNavigationEvent> = (
  value,
): value is EditorNavigationEvent =>
  shape(value, {
    type: (item) => item === "selectNode" || item === "revealNode",
    previewSessionId: nonEmptyString,
    renderRevision: (item) => safeInteger(item, 1),
    nodeId: nonEmptyString,
  });

export function isWebviewInboundMessage(
  value: unknown,
): value is WebviewInboundMessage {
  if (!object(value)) return false;
  if (value.type === "ready" || value.type === "requestSnapshot")
    return Object.keys(value).length === 1;
  if (value.type === "rendered")
    return shape(value, {
      type: (item) => item === "rendered",
      previewSessionId: nonEmptyString,
      renderRevision: (item) => safeInteger(item, 1),
    });
  return isEditorNavigationEvent(value);
}

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

export const isPreviewNavigationEvent: Validator<PreviewNavigationEvent> = (
  value,
): value is PreviewNavigationEvent => {
  if (!object(value)) return false;
  if (value.type === "selection")
    return shape(
      value,
      {
        type: (item) => item === "selection",
        previewSessionId: nonEmptyString,
        renderRevision: (item) => safeInteger(item, 1),
        nodeIds: uniqueNonEmptyStrings,
      },
      { activePosition: nullable(isPosition) },
    );
  return shape(value, {
    type: (item) => item === "viewport",
    previewSessionId: nonEmptyString,
    renderRevision: (item) => safeInteger(item, 1),
    nodeId: nonEmptyString,
  });
};

export const isSourceNavigationEvent: Validator<SourceNavigationEvent> = (
  value,
): value is SourceNavigationEvent =>
  shape(value, {
    type: (item) => item === "selectSource" || item === "revealSource",
    sourceRange: isSourceRange,
  });

export type PreviewHostEvent = RenderPublication | PreviewNavigationEvent;
export const isPreviewHostEvent: Validator<PreviewHostEvent> = (
  value,
): value is PreviewHostEvent =>
  isRenderPublication(value) || isPreviewNavigationEvent(value);

const isInitializeParams: Validator<InitializeParams> = (value) =>
  shape(
    value,
    {
      protocolVersion: (item) => item === 1,
      client: (item) => shape(item, { name: string, version: string }),
    },
    {
      capabilities: (item) =>
        shape(
          item,
          {},
          {
            embeddedHtml: boolean,
            structuredPreview: boolean,
            selectionEvents: boolean,
            viewportEvents: boolean,
            openExternal: boolean,
          },
        ),
      workspaces: array((item) =>
        shape(item, { uri: string, trusted: boolean }),
      ),
    },
  );

const isInitializeResult: Validator<InitializeResult> = (value) =>
  shape(value, {
    protocolVersion: (item) => item === 1,
    daemonInstanceId: string,
    workspaceStatuses: array((item) =>
      shape(item, { uri: string, enabled: boolean }, { error: string }),
    ),
    capabilities: (item) =>
      shape(item, {
        htmlRender: boolean,
        documentCheckpoint: boolean,
        selectionEvents: boolean,
        viewportEvents: boolean,
        workspaceCommands: array(string),
      }),
  });

const daemonId = { daemonInstanceId: string };
const sessionId = { documentSessionId: string };
const previewId = { previewSessionId: string };
const documentResult = {
  documentVersion: (item: unknown) => safeInteger(item),
  contentHash: string,
};
const isAttachResult: Validator<AttachDocumentResult> = (value) =>
  shape(value, { documentSessionId: string, ...documentResult });
const isCheckpointResult: Validator<CheckpointDocumentResult> = (value) =>
  shape(value, documentResult);

const isCommandResult: Validator<CommandResult> = (value) =>
  shape(
    value,
    {},
    {
      message: (item) =>
        shape(item, {
          level: (level) =>
            ["info", "warning", "error"].includes(level as string),
          text: string,
        }),
      openUri: string,
    },
  );

const nullResult: Validator<null> = (value): value is null => value === null;

export const customRequestParamsValidators: {
  [Method in keyof CustomRequestMap]: Validator<
    CustomRequestMap[Method]["params"]
  >;
} = {
  "fleximark/initialize": isInitializeParams,
  "fleximark/attachDocument": (value) =>
    shape(value, {
      ...daemonId,
      uri: string,
      expectedDocumentVersion: safeInteger,
      contentHash: string,
    }),
  "fleximark/checkpointDocument": (value) =>
    shape(value, {
      ...daemonId,
      ...sessionId,
      documentVersion: safeInteger,
      contentHash: string,
    }),
  "fleximark/render": (value) =>
    shape(value, {
      ...daemonId,
      ...sessionId,
      documentVersion: safeInteger,
    }),
  "fleximark/createPreview": (value) =>
    shape(value, {
      ...daemonId,
      ...sessionId,
      expectedDocumentVersion: safeInteger,
      target: (item) => item === "embeddedHtml" || item === "externalBrowser",
    }),
  "fleximark/disposePreview": (value) =>
    shape(value, { ...daemonId, ...previewId }),
  "fleximark/reloadPreview": (value) =>
    shape(value, { ...daemonId, ...previewId }),
  "fleximark/getNoteOptions": (value) =>
    shape(value, { ...daemonId, workspaceUri: string }),
  "fleximark/reconfigureWorkspace": (value) =>
    shape(value, { ...daemonId, workspaceUri: string, trusted: boolean }),
  "fleximark/executeCommand": (value) =>
    shape(
      value,
      { ...daemonId, command: string },
      {
        documentSessionId: string,
        expectedDocumentVersion: safeInteger,
        workspaceUri: string,
        destinationUri: string,
        noteCategory: string,
        noteTemplate: string,
      },
    ),
  "fleximark/openDocument": (value) =>
    shape(value, {
      ...daemonId,
      uri: string,
      documentVersion: safeInteger,
      text: string,
    }),
  "fleximark/changeDocument": (value) =>
    shape(value, {
      ...daemonId,
      ...sessionId,
      baseDocumentVersion: safeInteger,
      baseContentHash: string,
      documentVersion: safeInteger,
      text: string,
    }),
  "fleximark/closeDocument": (value) =>
    shape(value, { ...daemonId, ...sessionId }),
};

export const customRequestResultValidators: {
  [Method in keyof CustomRequestMap]: Validator<
    CustomRequestMap[Method]["result"]
  >;
} = {
  "fleximark/initialize": isInitializeResult,
  "fleximark/attachDocument": isAttachResult,
  "fleximark/checkpointDocument": isCheckpointResult,
  "fleximark/render": isRenderPublication,
  "fleximark/createPreview": (value) =>
    object(value) &&
    shape(
      value,
      { previewSessionId: string, initialPublication: isRenderSnapshot },
      { url: string },
    ) &&
    (value.initialPublication as RenderSnapshot).previewSessionId ===
      value.previewSessionId,
  "fleximark/disposePreview": nullResult,
  "fleximark/reloadPreview": nullResult,
  "fleximark/getNoteOptions": (value) =>
    shape(value, { categories: uniqueStrings, templates: uniqueStrings }),
  "fleximark/reconfigureWorkspace": nullResult,
  "fleximark/executeCommand": isCommandResult,
  "fleximark/openDocument": isAttachResult,
  "fleximark/changeDocument": isCheckpointResult,
  "fleximark/closeDocument": nullResult,
};

export const clientNotificationValidators: {
  [Method in keyof ClientNotificationMap]: Validator<
    ClientNotificationMap[Method]
  >;
} = {
  "fleximark/setSelection": (value) =>
    shape(value, {
      ...daemonId,
      ...sessionId,
      expectedDocumentVersion: safeInteger,
      selections: array(isTextSelection),
    }),
  "fleximark/setViewport": (value) =>
    shape(value, {
      ...daemonId,
      ...sessionId,
      expectedDocumentVersion: safeInteger,
      ranges: array(isTextRange),
    }),
  "fleximark/previewEvent": (value) =>
    object(value) &&
    shape(value, {
      ...daemonId,
      ...previewId,
      renderRevision: (item) => safeInteger(item, 1),
      event: isEditorNavigationEvent,
    }) &&
    (value.event as EditorNavigationEvent).previewSessionId ===
      value.previewSessionId &&
    (value.event as EditorNavigationEvent).renderRevision ===
      value.renderRevision,
};

const isRequestFullText: Validator<RequestFullTextParams> = (value) =>
  shape(value, { ...daemonId, uri: string, ...sessionId, reason: string });

const isServerPreviewEvent: Validator<ServerPreviewEventParams> = (value) =>
  object(value) &&
  shape(value, {
    ...daemonId,
    ...previewId,
    renderRevision: (item) => safeInteger(item, 1),
    event: (item) =>
      isRenderPublication(item) ||
      isPreviewNavigationEvent(item) ||
      isSourceNavigationEvent(item),
  }) &&
  (() => {
    const params = value as unknown as ServerPreviewEventParams;
    const event = params.event;
    if (event.type === "selectSource" || event.type === "revealSource")
      return true;
    const revision =
      event.type === "full" || event.type === "patch"
        ? event.resultRenderRevision
        : event.renderRevision;
    return (
      event.previewSessionId === params.previewSessionId &&
      revision === params.renderRevision
    );
  })();

export const isJsonValue: Validator<JsonValue> = (value): value is JsonValue =>
  value === null ||
  typeof value === "boolean" ||
  typeof value === "string" ||
  (typeof value === "number" && Number.isFinite(value)) ||
  (Array.isArray(value) && value.every(isJsonValue)) ||
  (object(value) && Object.values(value).every(isJsonValue));

const isJsonRpcId = (value: unknown): boolean =>
  string(value) || safeInteger(value);

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
      { jsonrpc: (item) => item === "2.0", id: isJsonRpcId },
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
  "fleximark/requestFullText": isRequestFullText,
  "fleximark/previewEvent": isServerPreviewEvent,
  "textDocument/publishDiagnostics": (value) =>
    shape(
      value,
      { uri: string, diagnostics: array(isDiagnostic) },
      { version: (item) => safeInteger(item) },
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
