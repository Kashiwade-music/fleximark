import { Node, Parent, Root } from "hast";
import { Plugin } from "unified";

/**
 * rehype plugin to remove all `position` properties from nodes.
 */
const rehypeRemovePosition: Plugin<[], Root> = () => {
  function strip(node: Node): void {
    if (node && "position" in node) {
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      delete (node as any).position;
    }

    const children = (node as Parent).children;
    if (children && children.length > 0) {
      for (const child of children) {
        strip(child);
      }
    }
  }

  return (tree: Root) => {
    strip(tree);
  };
};

export default rehypeRemovePosition;
