import { Root, RootContent } from "hast";
import { Plugin } from "unified";

interface RehypeLineNumberOptions {
  isNeedDataLineNumber?: boolean;
}

const rehypeLineNumber: Plugin<[RehypeLineNumberOptions?], Root> = (
  options = {},
) => {
  const { isNeedDataLineNumber = true } = options;

  if (isNeedDataLineNumber === false) {
    return () => {
      /* no-op */
    };
  }

  return (tree: Root) => {
    if (tree.type !== "root") return;

    tree.children.forEach((child: RootContent) => {
      if (child.type !== "element") return;

      child.properties ??= {};
      child.properties["data-line-number"] = String(
        child.position?.start.line || 0,
      );
    });
  };
};

export default rehypeLineNumber;
