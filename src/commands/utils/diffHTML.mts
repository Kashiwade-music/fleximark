import crypto from "crypto";
import { toHtml } from "hast-util-to-html";
import type { Root, RootContent } from "hast";

// Represents a change between two HTML trees
export interface HtmlEditScript {
  index: number;
  operation: "insert" | "delete" | "update";
  newHTMLHash: string;
  newHTML: string;
}

// Result of HTML diffing
export interface FindDiffResult {
  editScripts: HtmlEditScript[];
  dataLineArray: Array<Record<string, string>>;
}

/**
 * Computes the diff between two HAST trees.
 */
export function findDiff(beforeTree: Root, afterTree: Root): FindDiffResult {
  const { hashArray: beforeHashes } = hashHastContent(beforeTree.children);
  const {
    hashMap: afterHashMap,
    hashArray: afterHashes,
    dataLineArray: lineMetadata,
  } = hashHastContent(afterTree.children);

  let editScripts = computeEditScript(beforeHashes, afterHashes);

  // Attach rendered HTML for inserts/updates
  editScripts = editScripts.map((edit) => {
    const contentNode = afterHashMap[edit.newHTMLHash];
    const html = contentNode
      ? toHtml(contentNode, { allowDangerousHtml: true })
      : "";

    return { ...edit, newHTML: html };
  });

  return { editScripts, dataLineArray: lineMetadata };
}

type HashMap = Record<string, RootContent>;
type LineMetadataArray = Array<Record<string, string>>;

/**
 * Hashes each HAST node and collects relevant metadata.
 */
function hashHastContent(nodes: RootContent[]) {
  const hashMap: HashMap = {};
  const hashArray: string[] = [];
  const dataLineArray: LineMetadataArray = [];

  for (const node of nodes) {
    if (isIgnorableTextNode(node)) continue;

    const properties = (node as any).properties ?? {};
    const lineInfo = extractAndRemoveLineNumber(properties);

    dataLineArray.push(lineInfo);

    const serialized = JSON.stringify(node);
    const hash = crypto.createHash("sha256").update(serialized).digest("hex");

    hashMap[hash] = node;
    hashArray.push(hash);
  }

  return { hashMap, hashArray, dataLineArray };
}

/**
 * Returns true if the node is a pure newline text node (to be ignored).
 */
function isIgnorableTextNode(node: RootContent): boolean {
  return (
    typeof node === "object" &&
    node !== null &&
    (node as any).type === "text" &&
    (node as any).value === "\n"
  );
}

/**
 * Extracts and removes `data-line-number` from properties object.
 */
function extractAndRemoveLineNumber(
  properties: Record<string, unknown>
): Record<string, string> {
  const lineNumber = properties["data-line-number"];
  if (typeof lineNumber === "string") {
    delete properties["data-line-number"];
    return { "data-line-number": lineNumber };
  }
  return {};
}

/**
 * Computes Levenshtein-based edit script between two sequences of hash strings.
 */
function computeEditScript(
  source: string[],
  target: string[]
): HtmlEditScript[] {
  const m = source.length;
  const n = target.length;

  const dp: number[][] = Array.from({ length: m + 1 }, () =>
    Array(n + 1).fill(0)
  );
  const ops: ("none" | "insert" | "delete" | "update")[][] = Array.from(
    { length: m + 1 },
    () => Array(n + 1).fill("none")
  );

  // Initialize DP tables
  for (let i = 0; i <= m; i++) {
    dp[i][0] = i;
    ops[i][0] = "delete";
  }
  for (let j = 0; j <= n; j++) {
    dp[0][j] = j;
    ops[0][j] = "insert";
  }
  ops[0][0] = "none";

  // Compute edit distances
  for (let i = 1; i <= m; i++) {
    for (let j = 1; j <= n; j++) {
      if (source[i - 1] === target[j - 1]) {
        dp[i][j] = dp[i - 1][j - 1];
        ops[i][j] = "none";
      } else {
        const deleteCost = dp[i - 1][j] + 1;
        const insertCost = dp[i][j - 1] + 1;
        const updateCost = dp[i - 1][j - 1] + 1;

        dp[i][j] = Math.min(deleteCost, insertCost, updateCost);

        if (dp[i][j] === updateCost) {
          ops[i][j] = "update";
        } else if (dp[i][j] === insertCost) {
          ops[i][j] = "insert";
        } else {
          ops[i][j] = "delete";
        }
      }
    }
  }

  // Backtrack to generate edit script
  const edits: HtmlEditScript[] = [];
  let i = m;
  let j = n;

  while (i > 0 || j > 0) {
    const operation = ops[i][j];

    switch (operation) {
      case "none":
        i--;
        j--;
        break;
      case "update":
        edits.unshift({
          index: i - 1,
          operation: "update",
          newHTMLHash: target[j - 1],
          newHTML: "",
        });
        i--;
        j--;
        break;
      case "insert":
        edits.unshift({
          index: i,
          operation: "insert",
          newHTMLHash: target[j - 1],
          newHTML: "",
        });
        j--;
        break;
      case "delete":
        edits.unshift({
          index: i - 1,
          operation: "delete",
          newHTMLHash: "",
          newHTML: "",
        });
        i--;
        break;
    }
  }

  return edits;
}
