/** Shared object guard for all untrusted JSON and browser message boundaries. */
export const isRecord = (value: unknown): value is Record<string, unknown> =>
  value !== null && typeof value === "object" && !Array.isArray(value);
