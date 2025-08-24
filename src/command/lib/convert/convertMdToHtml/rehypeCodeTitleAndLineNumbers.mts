// plugins/rehypeCodeTitleAndLineNumbers.ts
import type { Element, Root } from "hast";
import { Plugin } from "unified";
import { visit } from "unist-util-visit";

const TITLE_RE = /(?:^|\s)title=(?:"([^"]*)"|'([^']*)')/;
const LNUM_RE = /(?:^|\s)showLineNumbers(?:=(\d+))?/;
const LNUM_ALT = /(?:^|\s)showLineNumber(?:=(\d+))?/; // 互換

const shouldProcess = (node: Element, parent: Element) => {
  if (!parent) return false;

  // 対象: <pre> 要素
  if (node.tagName !== "pre") return false;

  // すでに <figure> 配下の直下ならスキップ
  const isAlreadyInFigure =
    (parent as Element).type === "element" &&
    (parent as Element).tagName === "figure";
  if (isAlreadyInFigure) return false;

  // <pre> 直下の最初の子が <code> か検査
  const first = node.children?.[0];
  const hasCodeChild =
    first && first.type === "element" && (first as Element).tagName === "code";
  if (!hasCodeChild) return false;

  return true;
};

function countLines(obj?: { value?: unknown }): number {
  if (!obj) return 0;
  if (typeof obj.value === "string") {
    // 空文字列でも1行としてカウントするため split した結果の length を返す
    const splitted = obj.value.split(/\r\n|\r|\n/);
    if (splitted.length === 0) return 0;
    if (splitted[splitted.length - 1] === "") return splitted.length - 1;
    return splitted.length;
  }
  return 0;
}

const rehypeCodeTitleAndLineNumbers: Plugin<[], Root> = () => {
  return (tree: Root) => {
    visit(tree, "element", (node: Element, idx, parent) => {
      if (!shouldProcess(node, parent as Element)) return;

      const wrappedPre = structuredClone(node) as Element;
      const wrappedPreCode = wrappedPre.children?.[0] as Element;

      node = {
        type: "element",
        tagName: "figure",
        properties: { dataShikiFigure: "" },
        children: [wrappedPre],
        position: node.position,
      };

      // もし、wrappedPreにdata-line-numberがあれば引き継ぐ
      if (wrappedPre.properties?.["data-line-number"]) {
        node.properties = {
          ...node.properties,
          "data-line-number": wrappedPre.properties["data-line-number"],
        };
      }

      // 1) title=... を拾う
      const meta = wrappedPreCode.data?.meta ?? "";
      const titleMatch = meta.match(TITLE_RE);
      const title = (titleMatch?.[1] ?? titleMatch?.[2])?.trim();
      if (title) {
        const figcaption = {
          type: "element",
          tagName: "figcaption",
          properties: {
            dataShikiTitle: "",
            dataLanguage: "c++",
            dataTheme: "github-light-default",
          },
          children: [
            {
              type: "text",
              value: title,
            },
          ],
        };
        node.children = [figcaption as Element, ...node.children];
      }

      // 2) 行番号フラグ＆開始番号
      const startStr =
        meta.match(LNUM_RE)?.[1] ?? meta.match(LNUM_ALT)?.[1] ?? "";
      const hasLineNumbers = LNUM_RE.test(meta) || LNUM_ALT.test(meta);
      const start = startStr ? Math.max(1, parseInt(startStr, 10)) : 1;
      const codeLineLength = countLines(
        // eslint-disable-next-line @typescript-eslint/no-explicit-any
        (wrappedPreCode.children?.[0] as any) ?? {},
      );
      if (hasLineNumbers) {
        wrappedPreCode.properties = {
          ...wrappedPreCode.properties,
          dataShikiLineNumbers: "",
          dataShikiLineNumbersMaxDigits: String(
            String(start + codeLineLength - 1).length,
          ),
          style: "counter-reset: line " + (start - 1) + ";",
        };
      }

      // 3) Shiki に渡すメタから title/showLineNumbers 記法だけ除去
      wrappedPreCode.data = wrappedPreCode.data ?? {};
      wrappedPreCode.data.meta = meta
        .replace(TITLE_RE, " ")
        .replace(LNUM_RE, " ")
        .replace(LNUM_ALT, " ")
        .replace(/\s+/g, " ")
        .trim();

      // 親ノードの子を置き換え
      if (typeof idx === "number" && parent) {
        parent.children.splice(idx, 1, node);
      }
    });
  };
};

export default rehypeCodeTitleAndLineNumbers;
