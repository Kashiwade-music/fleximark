import type { SourcePosition } from "./protocol.mjs";

export function sourcePositionToCharacter(
  line: string,
  position: SourcePosition,
): number {
  if (position.encoding === "utf16") return position.character;
  let sourceUnits = 0;
  let utf16Units = 0;
  for (const character of line) {
    const next =
      sourceUnits +
      (position.encoding === "utf8" ? Buffer.byteLength(character, "utf8") : 1);
    if (next > position.character) break;
    sourceUnits = next;
    utf16Units += character.length;
  }
  return utf16Units;
}

export function sourcePositionWithinLine(
  line: string,
  position: SourcePosition,
): boolean {
  const length =
    position.encoding === "utf8"
      ? Buffer.byteLength(line, "utf8")
      : position.encoding === "utf32"
        ? [...line].length
        : line.length;
  return position.character <= length;
}
