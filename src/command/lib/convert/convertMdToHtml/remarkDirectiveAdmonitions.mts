import { Data, Node, Paragraph } from "mdast";
import { ContainerDirective } from "mdast-util-directive";
import { Plugin } from "unified";
import { visit } from "unist-util-visit";

const ALLOWED_TYPES = ["info", "tip", "warning", "danger"] as const;
type AllowedType = (typeof ALLOWED_TYPES)[number];

export interface ExtendedContainerDirective {
  /**
   * Node type of container directive.
   */
  type: "containerDirective";

  /**
   * Directive name.
   */
  name: string;

  /**
   * Directive attributes.
   */
  attributes?: Record<string, string | null | undefined> | null | undefined;

  /**
   * Children of container directive.
   */
  children: Node[];

  /**
   * Data associated with the mdast container directive.
   */
  data?: Data | undefined;
}

const remarkDirectiveAdmonitions: Plugin = () => {
  return (tree: Node) => {
    visit(tree, "containerDirective", (node: ContainerDirective) => {
      if (ALLOWED_TYPES.includes(node.name as AllowedType)) {
        // set parent
        const directive = node as ExtendedContainerDirective;
        directive.data ??= {};
        directive.data.hName = "div";
        directive.data.hProperties = {
          className: [directive.name],
        };

        // walk through children and separate title from content
        let titleNode = null;
        const contentNodes = [];

        for (const child of node.children) {
          if (
            child.type === "paragraph" &&
            child.data?.directiveLabel === true
          ) {
            // title node
            titleNode = {
              ...child,
              data: {
                ...child.data,
                hName: "div",
                hProperties: {
                  className: "admonition-title",
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
              hName: "div",
              hProperties: {
                className: "admonition-title",
              },
            },
            children: [
              {
                type: "html",
                value: getIconSvg(directive.name as AllowedType),
              },
              {
                type: "text",
                value:
                  " " +
                  directive.name.charAt(0).toUpperCase() +
                  directive.name.slice(1),
              },
            ],
          } as Paragraph;
        } else {
          titleNode.children.unshift({
            type: "html",
            value: getIconSvg(directive.name as AllowedType),
          });
        }

        // wrap contentNodes in a single admonition-content div
        const contentWrapper = {
          type: "containerDiv",
          data: {
            hName: "div",
            hProperties: {
              className: "admonition-content",
            },
          },
          children: contentNodes,
        };

        // set the final children array
        directive.children = [
          ...(titleNode ? [titleNode] : []),
          contentWrapper,
        ];
      }
    });
  };
};

function getIconSvg(type: AllowedType): string {
  // https://github.com/icons8/line-awesome
  switch (type) {
    case "info":
      return `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 32 32"><path d="M16 3a13 13 0 1 0 0 26 13 13 0 0 0 0-26Zm0 2a11 11 0 1 1 0 22 11 11 0 0 1 0-22Zm-1 5v2h2v-2Zm0 4v8h2v-8Z"/></svg>`;
    case "tip":
      return `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 32 32"><path d="M16 4c-5 0-9 4-9 9 0 2 .8 4 2 5.7.9 1.3 1.9 2.5 3 3.5V25c0 1 1 2 2 2l1 1h2l1-1c1 0 2-1 2-2v-2.8c1.1-1 2.1-2.2 3-3.5 1.2-1.8 2-3.8 2-5.7 0-5-4-9-9-9Zm0 2a7 7 0 0 1 7 7c0 1.3-.6 3-1.7 4.6a13 13 0 0 1-3.1 3.4h-4.4a13 13 0 0 1-3.1-3.4A9.3 9.3 0 0 1 9 13a7 7 0 0 1 7-7Zm-1.8 17h3.6l.2.1V25h-4v-1.9l.3-.1Z"/></svg>`;
    case "warning":
      return `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 32 32"><path d="m16 3.2-.9 1.5-12 20.8-.8 1.5h27.4l-.8-1.5-12-20.8Zm0 4L26.3 25H5.6ZM15 14v6h2v-6Zm0 7v2h2v-2Z"/></svg>`;
    case "danger":
      return `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 32 32"><path d="m16.8 4.4-3 5L12 7.5l-.8.8C7.8 12.3 6 16 6 19c0 5 4.5 9 10 9s10-4 10-9c0-4.8-5.2-10.6-8.3-13.7l-.9-1zm.4 3.2C19.8 10.3 24 15.3 24 19c0 2.4-1.4 4.5-3.5 5.8a6 6 0 0 0 .5-2.4c0-2.4-1.7-5.2-3.1-7.1l-.8-1.2-2.3 3.3-1.4-1.4-.6 1.1c-1.2 2-1.8 3.7-1.8 5.3 0 .9.2 1.7.5 2.4A6.8 6.8 0 0 1 8 19c0-2.4 1.4-5.3 4-8.5l2.2 2.1 3-5zm-.1 10a10 10 0 0 1 1.9 4.8c0 2-1.3 3.6-3 3.6s-3-1.6-3-3.6c0-.9.3-2 .9-3.1l1.3 1.3 1.9-3z"/></svg>`;
    default:
      return "";
  }
}

export default remarkDirectiveAdmonitions;
