import { Plugin } from "unified";
import { visit } from "unist-util-visit";
import { Root, RootContent } from "hast";

interface RehypeLineNumberOptions {
  isNeedDataLineNumber?: boolean;
}

const rehypeLineNumber: Plugin<[RehypeLineNumberOptions?], Root> = (
  options = {},
) => {
  const { isNeedDataLineNumber = true } = options;

  return (tree: Root) => {
    visit(tree, "root", (node: Root) => {
      if (!isNeedDataLineNumber) {
        // If data-line-number is not needed, return early
        return;
      }

      node.children.forEach((child: RootContent) => {
        if (!("position" in child)) return;
        if (child.type !== "element") return;

        child.properties ??= {};
        child.properties["data-line-number"] = String(
          child.position?.start.line || 0,
        );
      });
    });
  };
};

export default rehypeLineNumber;
