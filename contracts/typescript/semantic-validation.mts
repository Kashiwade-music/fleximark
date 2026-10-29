import type * as Contract from "./protocol.generated.mjs";
import type { TYPE_VALIDATORS } from "./protocol.generated.mjs";

type Entry = (typeof TYPE_VALIDATORS)[keyof typeof TYPE_VALIDATORS];
export type SemanticKind = Exclude<Entry["semantic"], null>;
const decodedBase64Length = (value: string): number | undefined => {
  if (value.length % 4 !== 0 || !/^[A-Za-z0-9+/]*={0,2}$(?![\s\S])/.test(value))
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
  new Set(assets.map(({ reference }) => reference)).size === assets.length;

const frameSemanticsAreValid = (frame: Contract.RenderFrame): boolean => {
  const blockIds = new Set(frame.blocks.map(({ id }) => id));
  const nodeIds = frame.blocks.flatMap(({ nodeIds }) => nodeIds);
  const identities = new Set(nodeIds);
  return (
    /^[0-9a-f]{64}$/.test(frame.rendererFingerprint) &&
    blockIds.size === frame.blocks.length &&
    identities.size === nodeIds.length &&
    frame.blocks.every(({ id, nodeIds: ids }) => ids.includes(id)) &&
    navigationIsValid(frame.navigation) &&
    new Set(frame.navigation.map(({ nodeId }) => nodeId)).size ===
      frame.navigation.length &&
    frame.navigation.every(({ nodeId }) => identities.has(nodeId)) &&
    assetsAreValid(frame.assets)
  );
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
  return false;
};

type FrameEnvelope = Pick<
  Contract.CreatePreviewResult,
  "frame" | "previewSessionId"
> &
  Partial<Pick<Contract.PreviewChangedParams, "renderRevision">>;
const frameEnvelopeIsValid = (value: FrameEnvelope): boolean =>
  frameSemanticsAreValid(value.frame) &&
  value.frame.previewSessionId === value.previewSessionId &&
  (value.renderRevision === undefined ||
    value.frame.renderRevision === value.renderRevision);

// Generated schema metadata is checked before this internal dispatch adapter.
// Its one type assertion connects the runtime schema registry to typed handlers.
const schemaSemantic =
  <T,>(check: (value: T) => boolean) =>
  (value: unknown): boolean =>
    check(value as T);

export const semanticHandlers = {
  frame: schemaSemantic(frameSemanticsAreValid),
  frameEnvelope: schemaSemantic(frameEnvelopeIsValid),
  previewEvent: schemaSemantic(previewEventParamsSemanticsAreValid),
  serverPreviewEvent: schemaSemantic(serverPreviewEventParamsSemanticsAreValid),
} satisfies Record<SemanticKind, (value: unknown) => boolean>;
