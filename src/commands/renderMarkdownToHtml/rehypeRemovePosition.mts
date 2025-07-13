import { Node, Root } from "hast";
import { Plugin } from "unified";
import { visit } from "unist-util-visit";

/**
 * rehype plugin to remove all `position` properties from nodes.
 */
const rehypeRemovePosition: Plugin<[], Root> = () => {
  return (tree: Root) => {
    visit(tree, (node: Node) => {
      if ("position" in node) {
        delete node.position;
      }
    });
  };
};

export default rehypeRemovePosition;
