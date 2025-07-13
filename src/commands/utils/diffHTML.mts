import crypto from "crypto";
import { toHtml } from "hast-util-to-html";
import type { Root, RootContent } from "hast";

/**
 * Represents a single edit operation needed to transform one HTML tree into another.
 *
 * @property index - The position in the original HTML tree where the edit should be applied.
 * @property operation - The type of change: 'insert', 'delete', or 'update'.
 * @property newHTMLHash - SHA-256 hash of the new HTML node content (used for identification).
 * @property newHTML - Serialized HTML string corresponding to the new content being inserted or updated.
 */
export interface HtmlEditScript {
  index: number;
  operation: "insert" | "delete" | "update";
  newHTMLHash: string;
  newHTML: string;
}

/**
 * Structure returned by the `findDiff` function.
 *
 * @property editScripts - List of operations (insertions, deletions, updates) required to transform the input HTML tree.
 * @property dataLineArray - Metadata for each node, including extracted line numbers for traceability.
 */
export interface FindDiffResult {
  editScripts: HtmlEditScript[];
  dataLineArray: Record<string, string>[];
}

/**
 * Computes the difference between two HAST (Hypertext Abstract Syntax Tree) trees,
 * producing a list of edit operations and relevant metadata for transforming one tree into another.
 *
 * This function compares the SHA-256 hash of each node's serialized content, computes the minimal set
 * of edits (inserts, deletes, updates) using a Levenshtein distance approach, and returns the full HTML
 * strings for each changed node.
 *
 * @param beforeTree - The original HAST tree.
 * @param afterTree - The updated HAST tree to compare against.
 * @returns An object containing the edit script (list of diffs) and associated node metadata.
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
type LineMetadataArray = Record<string, string>[];

/**
 * Generates SHA-256 hashes for each node in the HAST tree and extracts line number metadata.
 *
 * The function:
 * - Ignores ignorable nodes (e.g., pure newline text).
 * - Extracts and removes `data-line-number` metadata from each node.
 * - Serializes each node into a string and hashes it for structural comparison.
 *
 * @param nodes - Array of `RootContent` nodes (e.g., paragraphs, elements, text).
 * @returns An object containing:
 *   - `hashMap`: A map of each node's hash to the actual node.
 *   - `hashArray`: An ordered list of hashes representing the node sequence.
 *   - `dataLineArray`: Extracted metadata for each node (e.g., line numbers).
 */
function hashHastContent(nodes: RootContent[]) {
  const hashMap: HashMap = {};
  const hashArray: string[] = [];
  const dataLineArray: LineMetadataArray = [];

  for (const node of nodes) {
    if (isIgnorableTextNode(node)) continue;

    const hasProperties = !(
      node.type === "comment" ||
      node.type === "doctype" ||
      node.type === "text" ||
      node.type === "raw"
    );

    const properties = hasProperties ? node.properties : {};
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
 * Determines whether a HAST node should be ignored when computing diffs.
 * Specifically filters out pure newline text nodes which are often irrelevant to structural diffs.
 *
 * @param node - A single HAST node.
 * @returns `true` if the node is a newline-only text node, `false` otherwise.
 */
function isIgnorableTextNode(node: RootContent): boolean {
  return (
    typeof node === "object" &&
    node !== null &&
    node.type === "text" &&
    node.value === "\n"
  );
}

/**
 * Extracts the `data-line-number` property from a node's properties object
 * and removes it from the original properties to avoid contaminating the node's serialized hash.
 *
 * @param properties - A node's attribute/properties object.
 * @returns An object containing only the extracted `data-line-number`, if found.
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
 * Uses a dynamic programming algorithm based on Levenshtein distance to compute
 * the minimum sequence of operations (insert, delete, update) required to transform
 * one array of hash strings into another.
 *
 * @param source - Array of SHA-256 hashes representing the original tree nodes.
 * @param target - Array of SHA-256 hashes representing the updated tree nodes.
 * @returns A list of edit operations (`HtmlEditScript`) required to convert `source` into `target`.
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

  // Initialize base cases
  for (let i = 0; i <= m; i++) {
    dp[i][0] = i;
    ops[i][0] = "delete";
  }
  for (let j = 0; j <= n; j++) {
    dp[0][j] = j;
    ops[0][j] = "insert";
  }
  ops[0][0] = "none";

  // Fill DP and operation matrices
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

  // Backtrack through the DP matrix to produce the actual edit script
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
