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
  line: number, // 1-based number
  column: number, // 1-based number
): NodeMatchResult | null {
  for (const child of root.children) {
    //currently only support code block
    if (child.type !== "code") continue;

    const pos = child.position;
    if (!pos || !pos.start || !pos.end) continue;

    const { start, end } = pos;

    const isInLineRange = line >= start.line && line <= end.line;
    const isInColumnRange =
      (line !== start.line || column >= start.column) &&
      (line !== end.line || column <= end.column);

    if (isInLineRange && isInColumnRange) {
      const lineOffset = line - start.line;
      const valueLines = child.value.split(/\r\n|\r|\n/);

      let offsetInNode = 0;
      for (const [contentLineIndex, contentLine] of valueLines.entries()) {
        if (contentLineIndex + 1 === lineOffset) {
          offsetInNode = offsetInNode + column - 1;
          break;
        } else {
          offsetInNode = offsetInNode + contentLine.length + 1; // +1 for newline
        }
      }

      return {
        type: child.type,
        language: child.lang || undefined, // For code blocks
        startLine: start.line,
        offsetInNode,
      };
    }
  }

  return null;
}

export default getBlockLineAndOffset;
