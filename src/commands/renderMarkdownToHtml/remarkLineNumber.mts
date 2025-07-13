import { Plugin } from "unified";
import { visit } from "unist-util-visit";
import { Node } from "unist";
import { Root } from "mdast";

interface HDataNode extends Node {
  data?: {
    hName?: string;
    hProperties?: Record<string, unknown>;
  };
}

interface RemarkLineNumberOptions {
  isNeedDataLineNumber?: boolean;
}

const remarkLineNumber: Plugin<[RemarkLineNumberOptions?]> = (options = {}) => {
  const { isNeedDataLineNumber = true } = options;

  return (tree: Node) => {
    visit(tree, "root", (node: Root) => {
      if (!node.children) return;
      if (!isNeedDataLineNumber) {
        // If data-line-number is not needed, return early
        return;
      }

      node.children.forEach((child: HDataNode) => {
        child.data ??= {};
        child.data.hProperties ??= {};
        child.data.hProperties["data-line-number"] = String(
          child.position?.start.line || 0
        );
      });
    });
    console.log(tree);
  };
};

export default remarkLineNumber;
