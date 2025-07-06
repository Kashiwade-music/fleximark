import { Plugin } from "unified";
import { visit } from "unist-util-visit";
import { Node, Parent } from "unist";
import { Paragraph, Html, Root } from "mdast";

interface HDataNode extends Node {
  data?: {
    hName?: string;
    hProperties?: Record<string, any>;
  };
}

const remarkLineNumber: Plugin = () => {
  return (tree: Node) => {
    visit(tree, "root", (node: Root) => {
      if (!node.children) return;

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
