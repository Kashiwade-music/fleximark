const PATCH_ATTRIBUTE_NAMES = new Set([
  "role",
  "aria-checked",
  "open",
  "class",
  "data-line-numbers",
  "data-admonition-kind",
]);

export function isSafePatchAttribute(
  name: string,
  value: unknown,
): value is string | null {
  return (
    PATCH_ATTRIBUTE_NAMES.has(name) &&
    (value === null || (typeof value === "string" && value.length <= 4096))
  );
}
