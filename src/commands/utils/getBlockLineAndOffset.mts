import { Root } from "mdast";

interface NodeMatchResult {
  type: string;
  language?: string; // For code blocks, e.g. "javascript"
  startLine: number;
  offsetInNode: number;
}

function getBlockLineAndOffset(
  root: Root,
  source: string,
  line: number,
  column: number,
): NodeMatchResult | null {
  // Preprocess: split into lines
  const lines = source.split(/\r\n|\r|\n/);

  for (const child of root.children) {
    const pos = child.position;
    if (!pos || !pos.start || !pos.end) continue;

    const { start, end } = pos;

    const isInLineRange = line >= start.line && line <= end.line;
    const isInColumnRange =
      (line !== start.line || column >= start.column) &&
      (line !== end.line || column <= end.column);

    if (isInLineRange && isInColumnRange) {
      const absoluteOffset = computeAbsoluteOffset({ line, column }, lines);
      const nodeStartOffset = computeAbsoluteOffset(start, lines);
      const nodeEndOffset = computeAbsoluteOffset(end, lines);

      const nodeText = source.slice(nodeStartOffset, nodeEndOffset);

      const relativeOffsetInNode = absoluteOffset - nodeStartOffset;

      // Now compute offset into content (excluding syntax)
      const contentOffset = computeOffsetInContentOnly(
        nodeText,
        relativeOffsetInNode,
        child.type,
      );

      if (child.type === "code") {
        return {
          type: child.type,
          language: child.lang || undefined, // For code blocks
          startLine: start.line,
          offsetInNode: contentOffset,
        };
      } else {
        return {
          type: child.type,
          startLine: start.line,
          offsetInNode: contentOffset,
        };
      }
    }
  }

  return null;
}

export default getBlockLineAndOffset;

// Calculates the offset from start of file to given position
function computeAbsoluteOffset(
  pos: { line: number; column: number },
  lines: string[],
): number {
  let offset = 0;
  for (let i = 0; i < pos.line - 1; i++) {
    offset += lines[i].length + 1; // +1 for newline
  }
  offset += pos.column - 1;
  return offset;
}

// Extracts offset within "content" (excluding syntax) for known node types
function computeOffsetInContentOnly(
  nodeText: string,
  rawOffset: number,
  nodeType: string,
): number {
  switch (nodeType) {
    case "code": {
      // Code block: remove ```lang\r?\n and ending ```
      const match = nodeText.match(
        /(^|\n)(`{3,})([^\n\r]*)[\r\n]+([\s\S]*?)\r?\n\2(?!`)/,
      );
      if (!match) return 0;

      const contentStartIndex = nodeText.indexOf(match[4]);
      return Math.max(0, rawOffset - contentStartIndex);
    }

    case "heading": {
      // Remove leading hashes and space (e.g. ## Heading)
      const match = nodeText.match(/^#+\s*/);
      const contentStart = match ? match[0].length : 0;
      return Math.max(0, rawOffset - contentStart);
    }

    case "blockquote": {
      // Remove leading "> " on each line
      const lines = nodeText.split(/\r?\n/);
      let offset = 0;
      let contentOffset = 0;
      for (const line of lines) {
        const stripped = line.replace(/^>\s?/, "");
        const lineStart = offset;
        const lineEnd = offset + line.length + 1; // +1 for newline
        if (rawOffset >= lineStart && rawOffset <= lineEnd) {
          contentOffset +=
            rawOffset - lineStart - (line.length - stripped.length);
          break;
        }
        offset += line.length + 1;
        contentOffset += stripped.length + 1;
      }
      return Math.max(0, contentOffset);
    }

    default:
      // Fallback: use rawOffset directly
      return rawOffset;
  }
}
