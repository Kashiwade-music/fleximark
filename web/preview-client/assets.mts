import type { RenderAsset } from "./protocol.mjs";

export function validAssets(assets: readonly RenderAsset[]): boolean {
  const references = new Set<string>();
  let total = 0;
  try {
    return assets.every((asset) => {
      const bytes = atob(asset.data);
      total += bytes.length;
      return (
        /^fleximark-asset:[0-9a-f]{64}$/.test(asset.reference) &&
        /^[0-9a-f]{64}$/.test(asset.contentHash) &&
        asset.reference === `fleximark-asset:${asset.contentHash}` &&
        /^(?:image\/(?:png|jpeg|gif|webp)|audio\/(?:mpeg|ogg|wav))$/.test(
          asset.mediaType,
        ) &&
        Number.isSafeInteger(asset.byteLength) &&
        asset.byteLength === bytes.length &&
        asset.byteLength <= 1024 * 1024 &&
        total <= 8 * 1024 * 1024 &&
        !references.has(asset.reference) &&
        (references.add(asset.reference), true)
      );
    });
  } catch {
    return false;
  }
}

export function createAssetUrls(
  assets: readonly RenderAsset[],
): Map<string, string> {
  const urls = new Map<string, string>();
  try {
    for (const asset of assets) {
      const binary = atob(asset.data);
      const bytes = Uint8Array.from(binary, (character) =>
        character.charCodeAt(0),
      );
      urls.set(
        asset.reference,
        URL.createObjectURL(new Blob([bytes], { type: asset.mediaType })),
      );
    }
    return urls;
  } catch (error) {
    revokeAssetUrls(urls.values());
    throw error;
  }
}

export function resolveAssetUrls(
  root: ParentNode,
  urls: ReadonlyMap<string, string>,
): boolean {
  const elements =
    root instanceof HTMLElement && root.matches("[src], [href], [xlink\\:href]")
      ? [root]
      : [];
  elements.push(
    ...root.querySelectorAll<HTMLElement>("[src], [href], [xlink\\:href]"),
  );
  for (const element of elements) {
    for (const name of ["src", "href", "xlink:href"]) {
      const value = element.getAttribute(name);
      if (!value?.startsWith("fleximark-asset:")) continue;
      const url = urls.get(value);
      if (!url) return false;
      element.setAttribute(name, url);
    }
  }
  return true;
}

export function revokeAssetUrls(urls: Iterable<string>): void {
  for (const url of urls) {
    try {
      URL.revokeObjectURL(url);
    } catch {
      // Continue revoking the remaining URLs.
    }
  }
}
