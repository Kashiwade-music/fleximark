import { Root } from "hast";
import crypto from "crypto";
import { toHtml } from "hast-util-to-html";

// html edit script for HTML diffing
export interface HtmlEditScript {
  index: number;
  operation: "insert" | "delete" | "update";
  newHTMLHash: string;
  newHTML: string;
}

export function findDiff(beforeHast: Root, afterHast: Root): HtmlEditScript[] {
  const { hashArray: beforeHashedArray } = createHashedObjectMap(
    beforeHast.children
  );
  const { hashMap: afterHashedMap, hashArray: afterHashedArray } =
    createHashedObjectMap(afterHast.children);

  let editScripts = levenshteinEditScript(beforeHashedArray, afterHashedArray);

  editScripts = editScripts.map((edit) => {
    const node = afterHashedMap[edit.newHTMLHash];
    const newHTML = node ? toHtml(node) : undefined;

    return {
      ...edit,
      newHTML: newHTML || "",
    };
  });

  return editScripts;
}

type HashMap<T> = { [hash: string]: T };

function createHashedObjectMap<T>(items: T[]) {
  const hashMap: HashMap<T> = {};
  const hashArray: string[] = [];

  items.forEach((item) => {
    // item が { type: "text", value: "\n" } ならスキップ
    if (
      typeof item === "object" &&
      item !== null &&
      (item as any).type === "text" &&
      (item as any).value === "\n"
    ) {
      return;
    }

    const dataToHash = `${JSON.stringify(item)}`;
    const hash = crypto.createHash("sha256").update(dataToHash).digest("hex");
    hashMap[hash] = item;
    hashArray.push(hash);
  });

  return { hashMap, hashArray };
}

function levenshteinEditScript(
  source: string[],
  target: string[]
): HtmlEditScript[] {
  const m = source.length;
  const n = target.length;

  // DPテーブルの初期化
  const dp: number[][] = Array.from({ length: m + 1 }, () =>
    Array(n + 1).fill(0)
  );
  const op: ("none" | "insert" | "delete" | "update")[][] = Array.from(
    { length: m + 1 },
    () => Array(n + 1).fill("none")
  );

  for (let i = 0; i <= m; i++) (dp[i][0] = i), (op[i][0] = "delete");
  for (let j = 0; j <= n; j++) (dp[0][j] = j), (op[0][j] = "insert");
  op[0][0] = "none";

  // DPテーブルの構築
  for (let i = 1; i <= m; i++) {
    for (let j = 1; j <= n; j++) {
      if (source[i - 1] === target[j - 1]) {
        dp[i][j] = dp[i - 1][j - 1];
        op[i][j] = "none";
      } else {
        const del = dp[i - 1][j] + 1;
        const ins = dp[i][j - 1] + 1;
        const sub = dp[i - 1][j - 1] + 1;

        dp[i][j] = Math.min(del, ins, sub);

        if (dp[i][j] === sub) op[i][j] = "update";
        else if (dp[i][j] === ins) op[i][j] = "insert";
        else op[i][j] = "delete";
      }
    }
  }

  // 編集コマンドのバックトラック
  const edits: HtmlEditScript[] = [];
  let i = m,
    j = n;

  while (i > 0 || j > 0) {
    const operation = op[i][j];
    if (operation === "none") {
      i--;
      j--;
    } else if (operation === "update") {
      edits.unshift({
        index: i - 1,
        operation: "update",
        newHTMLHash: target[j - 1],
        newHTML: "",
      });
      i--;
      j--;
    } else if (operation === "insert") {
      edits.unshift({
        index: i,
        operation: "insert",
        newHTMLHash: target[j - 1],
        newHTML: "",
      });
      j--;
    } else if (operation === "delete") {
      edits.unshift({
        index: i - 1,
        operation: "delete",
        newHTMLHash: "",
        newHTML: "",
      });
      i--;
    }
  }

  return edits;
}
