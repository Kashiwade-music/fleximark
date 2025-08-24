import rehypeShiki from "@shikijs/rehype";
import {
  transformerMetaHighlight,
  transformerMetaWordHighlight,
  transformerNotationDiff,
  transformerNotationHighlight,
} from "@shikijs/transformers";
import type { Element, Node, Root } from "hast";
import crypto from "node:crypto";
import type { Plugin } from "unified";
import { unified } from "unified";

import rehypeRemovePosition from "./rehypeRemovePosition.mjs";

/**
 * LRU メモリキャッシュ（依存なし実装）
 */
class LRUCache<K, V> {
  private map = new Map<K, V>();
  constructor(private readonly maxEntries = 200) {
    if (maxEntries <= 0) throw new Error("maxEntries must be > 0");
  }
  get(key: K): V | undefined {
    const v = this.map.get(key);
    if (v === undefined) return undefined;
    // 最近参照に更新
    this.map.delete(key);
    this.map.set(key, v);
    return v;
  }
  set(key: K, value: V) {
    if (this.map.has(key)) this.map.delete(key);
    this.map.set(key, value);
    // 溢れたら最古を削除
    if (this.map.size > this.maxEntries) {
      const oldest = this.map.keys().next().value;
      if (oldest !== undefined) this.map.delete(oldest);
    }
  }
  clear() {
    this.map.clear();
  }
}

function deepClone<T>(obj: T): T {
  return structuredClone(obj);
}

interface JsonObject {
  [key: string]: JsonValue;
}
type JsonValue = string | number | boolean | null | JsonObject | JsonValue[];

function stableStringify(obj: JsonObject): string {
  function sortObject(o: JsonValue): JsonValue {
    if (Array.isArray(o)) {
      // 配列はそのまま
      return o;
    } else if (o !== null && typeof o === "object") {
      // オブジェクトの場合
      const sortedKeys = Object.keys(o).sort();
      const result: JsonObject = {};
      for (const key of sortedKeys) {
        const value = (o as JsonObject)[key];
        result[key] = sortObject(value);
      }
      return result;
    } else {
      // プリミティブはそのまま
      return o;
    }
  }
  const sortedObj = sortObject(obj) as JsonObject;
  return JSON.stringify(sortedObj);
}

function sha256(input: string): string {
  return crypto.createHash("sha256").update(input).digest("hex");
}

const shikiProcessor = unified().use(rehypeShiki, {
  theme: "github-light-default",
  keepBackground: true,
  transformers: [
    transformerNotationHighlight(),
    transformerMetaHighlight(), // /{3,5-8}/ などの行ハイライト
    transformerMetaWordHighlight(), // /pattern/ ワード強調（任意）
    transformerNotationDiff(), // [!code ++] 等の diff 記法（任意）
  ],
});

const removePositionProcessor = unified().use(rehypeRemovePosition);

const wrapAsRoot = (node: Element): Root => {
  return { type: "root", children: [node] };
};

const unwrapFromRoot = (root: Root): Element => {
  if (root.children.length !== 1) {
    throw new Error("Expected root to have exactly one child element");
  }
  const child = root.children[0];
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  if ((child.type as any) === "root") {
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    return unwrapFromRoot(child as any);
  }
  return child as Element;
};

const shouldProcess = (node: Element, parent: Element) => {
  if (!parent) return false;

  // 対象: <pre> 要素
  if (node.tagName !== "pre") return false;

  // <figure> 配下の直下でないならスキップ
  const isAlreadyInFigure =
    (parent as Element).type === "element" &&
    (parent as Element).tagName === "figure";
  if (!isAlreadyInFigure) return false;

  // <pre> 直下の最初の子が <code> か検査
  const first = node.children?.[0];
  const hasCodeChild =
    first && first.type === "element" && (first as Element).tagName === "code";
  if (!hasCodeChild) return false;

  return true;
};

// key is hash of pre > code element
const cache = new LRUCache<string, Element>(100);

const rehypeShikiCached: Plugin<[], Root> = () => {
  return async (tree: Root) => {
    const nodesToProcess: { node: Element; idx: number; parent: Element }[] =
      [];

    (function visit(node: Node, idx?: number, parent?: Node) {
      if (node.type !== "element") {
        (node as Element).children?.forEach((child, i) =>
          visit(child, i, node),
        );
        return;
      }

      const element = node as Element;

      if (
        !shouldProcess(element, parent as Element) ||
        idx === undefined ||
        parent === undefined
      ) {
        element.children?.forEach((child, i) => visit(child, i, node));
        return;
      }

      nodesToProcess.push({ node: element, idx, parent: parent as Element });
    })(tree);

    const promises = nodesToProcess.map(async ({ node, idx, parent }) => {
      const preElement = unwrapFromRoot(
        await removePositionProcessor.run(wrapAsRoot(deepClone(node))),
      );

      const hashKey = sha256(
        stableStringify(preElement as unknown as JsonObject),
      );
      let highlightedTree = cache.get(hashKey);

      if (!highlightedTree) {
        highlightedTree = unwrapFromRoot(
          await shikiProcessor.run(wrapAsRoot(preElement)),
        );

        // preElement.children[0] は code 要素
        // preElement.children[0].properties が 上書きされていることがあるので復元
        (highlightedTree.children[0] as Element).properties = {
          ...(preElement.children[0] as Element).properties,
        };

        cache.set(hashKey, highlightedTree);
      }

      // 元の tree に反映
      if (parent.type === "element" && typeof idx === "number") {
        (parent.children as Element[])[idx] = highlightedTree;
      }
    });

    await Promise.all(promises);
  };
};

export default rehypeShikiCached;
