import { Plugin } from "unified";
import { visit } from "unist-util-visit";
import { ContainerDirective } from "mdast-util-directive";
import { Node } from "unist";

const ALLOWED_TYPES = ["info", "tip", "warning", "danger"] as const;

const remarkDirectiveDetails: Plugin = () => {
  return (tree: Node) => {
    visit(tree, (node: Node) => {
      if (
        node.type === "containerDirective" &&
        (node as ContainerDirective).name === "details"
      ) {
        // set parent
        const directive = node as ContainerDirective;
        directive.data ??= {};
        directive.data.hName = "details";
        directive.data.hProperties = {
          className: [directive.name],
        };

        // walk through children and separate title from content
        let titleNode = null;
        const contentNodes = [];

        for (const child of directive.children) {
          if (
            child.type === "paragraph" &&
            child.data?.directiveLabel === true
          ) {
            // title node
            titleNode = {
              ...child,
              data: {
                ...child.data,
                hName: "summary",
                hProperties: {
                  className: "details-title",
                },
              },
            };
          } else {
            // content node
            contentNodes.push(child);
          }
        }

        // if no titleNode, create a default one
        if (!titleNode) {
          titleNode = {
            type: "paragraph",
            data: {
              hName: "summary",
              hProperties: {
                className: "details-title",
              },
            },
            children: [
              {
                type: "text",
                value: "Details",
              },
            ],
          };
        }

        // wrap contentNodes in a single admonition-content div
        const contentWrapper = {
          type: "div",
          data: {
            hName: "div",
            hProperties: {
              className: "details-content",
            },
          },
          children: contentNodes,
        };

        // set the final children array

        directive.children = [
          // @ts-ignore
          ...(titleNode ? [titleNode] : []),
          // @ts-ignore
          contentWrapper,
        ];
      }
    });
  };
};

export default remarkDirectiveDetails;
