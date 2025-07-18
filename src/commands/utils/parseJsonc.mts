/* eslint-disable @typescript-eslint/no-explicit-any */

function parseJsonc(jsonString: string): any {
  let result = "";
  let inString = false;
  let inSingleLineComment = false;
  let inMultiLineComment = false;
  let prevChar = "";

  for (let i = 0; i < jsonString.length; i++) {
    const char = jsonString[i];
    const nextChar = jsonString[i + 1];

    if (inSingleLineComment) {
      if (char === "\n") {
        inSingleLineComment = false;
        result += char;
      }
      continue;
    }

    if (inMultiLineComment) {
      if (char === "*" && nextChar === "/") {
        inMultiLineComment = false;
        i++; // skip '/'
      }
      continue;
    }

    if (!inString && char === "/" && nextChar === "/") {
      inSingleLineComment = true;
      i++; // skip next '/'
      continue;
    }

    if (!inString && char === "/" && nextChar === "*") {
      inMultiLineComment = true;
      i++; // skip next '*'
      continue;
    }

    if (char === '"' && prevChar !== "\\") {
      inString = !inString;
    }

    result += char;
    prevChar = char;
  }

  try {
    return JSON.parse(result);
  } catch (error) {
    console.error("Failed to parse JSONC:", error);
    return {};
  }
}

export default parseJsonc;
