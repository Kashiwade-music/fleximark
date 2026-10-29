import type { RenderAsset, RenderFrame } from "@fleximark/preview/index";

export function assetBlock(
  id: string,
  reference: string,
): RenderFrame["blocks"][number] {
  return {
    id,
    nodeIds: [id],
    html: `<p data-fleximark-node-id="${id}"><img src="${reference}"></p>`,
  };
}

export function asset(data: string, contentHash: string): RenderAsset {
  return {
    reference: `fleximark-asset:${contentHash}`,
    mediaType: "image/png",
    contentHash,
    byteLength: 1,
    data,
  };
}

export function objectUrls(create?: typeof URL.createObjectURL) {
  const originalCreate = URL.createObjectURL;
  const originalRevoke = URL.revokeObjectURL;
  const created: string[] = [];
  const revoked: string[] = [];
  URL.createObjectURL = (blob) => {
    const url = create ? create(blob) : `blob:${created.length + 1}`;
    created.push(url);
    return url;
  };
  URL.revokeObjectURL = (url) => revoked.push(url);
  return {
    created,
    revoked,
    dispose() {
      URL.createObjectURL = originalCreate;
      URL.revokeObjectURL = originalRevoke;
    },
  };
}
