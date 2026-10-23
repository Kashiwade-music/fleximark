export function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

const SENSITIVE_KEYS = ["authorization", "token", "secret"] as const;

interface SensitiveKeyMatch {
  index: number;
  key: (typeof SENSITIVE_KEYS)[number];
}

function nextSensitiveKey(
  message: string,
  start: number,
): SensitiveKeyMatch | undefined {
  const match = /authorization|token|secret/i.exec(message.slice(start));
  if (!match) return;
  const key = match[0].toLowerCase();
  if (!SENSITIVE_KEYS.includes(key as SensitiveKeyMatch["key"])) return;
  return {
    index: start + match.index,
    key: key as SensitiveKeyMatch["key"],
  };
}

function isSeparator(character: string | undefined): boolean {
  return (
    character === "=" ||
    character === ":" ||
    character === " " ||
    character === "\t"
  );
}

function quotedEnd(
  message: string,
  opening: number,
  quote: string,
): number | undefined {
  let index = opening + 1;
  while (index < message.length) {
    const character = message[index];
    if (character === "\r" || character === "\n") return undefined;
    if (character === "\\") {
      index += index + 1 < message.length ? 2 : 1;
      continue;
    }
    if (character === quote) return index + 1;
    index += 1;
  }
  return undefined;
}

function isIdentifierStart(character: string | undefined): boolean {
  return character !== undefined && /[A-Za-z_$]/.test(character);
}

function isIdentifierPart(character: string | undefined): boolean {
  return character !== undefined && /[A-Za-z0-9_$.-]/.test(character);
}

function startsStructuredField(message: string, comma: number): boolean {
  let index = comma + 1;
  while (message[index] === " " || message[index] === "\t") index += 1;
  const quote = message[index];
  if (quote === '"' || quote === "'") {
    const end = quotedEnd(message, index, quote);
    if (end === undefined) return false;
    index = end;
  } else {
    if (!isIdentifierStart(message[index])) return false;
    index += 1;
    while (isIdentifierPart(message[index])) index += 1;
  }
  while (message[index] === " " || message[index] === "\t") index += 1;
  return message[index] === ":";
}

function authorizationValueEnd(message: string, start: number): number {
  let index = start;
  while (index < message.length) {
    const character = message[index];
    if (
      character === "\r" ||
      character === "\n" ||
      character === "}" ||
      character === "]"
    )
      return index;
    if (character === "," && startsStructuredField(message, index))
      return index;
    index += 1;
  }
  return index;
}

function tokenValueEnd(message: string, start: number): number {
  let index = start;
  while (index < message.length) {
    const character = message[index];
    if (
      character === " " ||
      character === "\t" ||
      character === "\r" ||
      character === "\n" ||
      character === "," ||
      character === "}" ||
      character === "]"
    )
      return index;
    index += 1;
  }
  return index;
}

export function redactSensitiveText(message: string): string {
  const output: string[] = [];
  let cursor = 0;
  while (cursor < message.length) {
    const match = nextSensitiveKey(message, cursor);
    if (!match) {
      output.push(message.slice(cursor));
      break;
    }
    output.push(message.slice(cursor, match.index));
    let separator = match.index + match.key.length;
    if (message[separator] === '"' || message[separator] === "'")
      separator += 1;
    const separatorStart = separator;
    while (isSeparator(message[separator])) separator += 1;
    if (separator === separatorStart) {
      output.push(message[match.index]);
      cursor = match.index + 1;
      continue;
    }
    output.push(message.slice(match.index, separator));
    const quote = message[separator];
    if (quote === '"' || quote === "'") {
      const end = quotedEnd(message, separator, quote);
      output.push(quote, "<redacted>");
      if (end === undefined) {
        const lineEnd = message.slice(separator).search(/[\r\n]/);
        cursor = lineEnd < 0 ? message.length : separator + lineEnd;
        continue;
      }
      output.push(quote);
      cursor = end;
      continue;
    }
    output.push("<redacted>");
    cursor =
      match.key === "authorization"
        ? authorizationValueEnd(message, separator)
        : tokenValueEnd(message, separator);
  }
  return output.join("");
}
