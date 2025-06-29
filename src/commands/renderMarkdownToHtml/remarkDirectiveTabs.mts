import { Plugin } from "unified";
import { visit } from "unist-util-visit";
import { ContainerDirective } from "mdast-util-directive";
import { Node, Parent } from "unist";

let tabIdCounter = 0;

const remarkDirectiveTabs: Plugin = () => {
  return (tree: Node) => {
    visit(tree, "containerDirective", (node: Node, index, parent) => {
      const directive = node as ContainerDirective;

      if (directive.name !== "tabs") return;

      // set root element
      directive.data ??= {};
      directive.data.hName = "div";
      directive.data.hProperties = {
        className: ["tabs-container"],
      };
    });
  };
};

export default remarkDirectiveTabs;
