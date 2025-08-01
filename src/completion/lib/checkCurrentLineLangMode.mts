import { Code } from "mdast";
import remarkParse from "remark-parse";
import { unified } from "unified";
import { EXIT, visit } from "unist-util-visit";

function checkCurrentLineLangMode(
  markdown: string,
  line: number,
): string | undefined {
  const tree = unified().use(remarkParse).parse(markdown);

  let foundLang: string | undefined = undefined;

  visit(tree, "code", (node: Code) => {
    if (node.position) {
      const { start, end } = node.position;
      if (start.line <= line && line <= end.line) {
        foundLang = node.lang || "plaintext";
        return EXIT;
      }
    }
  });

  return foundLang;
}

export default checkCurrentLineLangMode;
