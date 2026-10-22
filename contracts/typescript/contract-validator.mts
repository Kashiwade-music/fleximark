/**
 * Evaluates the compact, Rust-generated contract descriptor pool. This module
 * deliberately contains no FlexiMark method names, DTO fields, or wire rules.
 */
const object = (value: unknown): value is Record<string, unknown> =>
  value !== null && typeof value === "object" && !Array.isArray(value);

function validate(
  pool: readonly unknown[],
  index: number,
  value: unknown,
  depth: number,
): boolean {
  if (depth > 128) return false;
  const descriptor = pool[index];
  if (descriptor === "z") return value === null;
  if (descriptor === "b") return typeof value === "boolean";
  if (descriptor === "v") return true;
  const parts = descriptor as readonly unknown[];

  switch (parts[0]) {
    case "c":
      return value === parts[1];
    case "e":
      return parts.slice(1).includes(value);
    case "u":
      return parts
        .slice(1)
        .some((child) => validate(pool, child as number, value, depth + 1));
    case "l":
      return parts
        .slice(1)
        .every((child) => validate(pool, child as number, value, depth + 1));
    case "i":
      return (
        Number.isSafeInteger(value) &&
        (typeof parts[1] !== "number" || (value as number) >= parts[1]) &&
        (typeof parts[2] !== "number" || (value as number) <= parts[2])
      );
    case "n":
      return (
        typeof value === "number" &&
        Number.isFinite(value) &&
        (typeof parts[1] !== "number" || value >= parts[1]) &&
        (typeof parts[2] !== "number" || value <= parts[2])
      );
    case "s":
      return (
        typeof value === "string" &&
        (typeof parts[1] !== "string" ||
          new RegExp(parts[1], "u").test(value)) &&
        (typeof parts[2] !== "number" || value.length >= parts[2]) &&
        (typeof parts[3] !== "number" || value.length <= parts[3])
      );
    case "a": {
      if (!Array.isArray(value)) return false;
      return (
        (typeof parts[2] !== "number" || value.length >= parts[2]) &&
        (typeof parts[3] !== "number" || value.length <= parts[3]) &&
        (parts[4] !== true ||
          new Set(value.map((item) => JSON.stringify(item))).size ===
            value.length) &&
        value.every((item) =>
          validate(pool, parts[1] as number, item, depth + 1),
        )
      );
    }
    case "o": {
      if (!object(value)) return false;
      const required = parts[1] as Readonly<Record<string, number>>;
      const optional = parts[2] as Readonly<Record<string, number>>;
      for (const [name, child] of Object.entries(required)) {
        if (
          !Object.hasOwn(value, name) ||
          !validate(pool, child, value[name], depth + 1)
        )
          return false;
      }
      for (const [name, child] of Object.entries(optional)) {
        if (
          Object.hasOwn(value, name) &&
          !validate(pool, child, value[name], depth + 1)
        )
          return false;
      }
      for (const [name, item] of Object.entries(value)) {
        if (Object.hasOwn(required, name) || Object.hasOwn(optional, name))
          continue;
        if (
          parts[3] === undefined ||
          !validate(pool, parts[3] as number, item, depth + 1)
        )
          return false;
      }
      return (
        parts[4] === undefined ||
        Object.keys(value).every((name) =>
          validate(pool, parts[4] as number, name, depth + 1),
        )
      );
    }
    default:
      return false;
  }
}

export function validateContractDescriptor(
  pool: readonly unknown[],
  index: number,
  value: unknown,
): boolean {
  return validate(pool, index, value, 0);
}
