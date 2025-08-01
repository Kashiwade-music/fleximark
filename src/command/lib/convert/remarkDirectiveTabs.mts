import crypto from "crypto";
import { ContainerDirective } from "mdast-util-directive";
import { Plugin } from "unified";
import { Node } from "unist";
import { visit } from "unist-util-visit";

function objectToHash(obj: object): string {
  const jsonString = JSON.stringify(obj, Object.keys(obj).sort());
  return crypto.createHash("sha256").update(jsonString).digest("hex");
}

const remarkDirectiveTabs: Plugin = () => {
  let totalTabs = 0;

  return (tree: Node) => {
    visit(tree, "containerDirective", (node: Node) => {
      const root = node as ContainerDirective;

      if (root.name !== "tabs") return;

      root.data ??= {};
      root.data.hName = "div";
      root.data.hProperties = { className: ["tabs-container"] };

      const tabDirectives: ContainerDirective[] = root.children.filter(
        (child): child is ContainerDirective =>
          child.type === "containerDirective" && child.name === "tab",
      );

      const uid =
        "tab-" +
        objectToHash({
          totalTabs,
          rootName: root.name,
          token: "EPj1fuyZ1V",
        });
      const inputs = generateTabInputs(tabDirectives.length, uid);
      const labels = generateTabLabels(tabDirectives, uid);
      const contents = generateTabContents(tabDirectives, uid);
      const styles = generateTabStyles(tabDirectives.length, uid);

      // @ts-expect-error keep flexibility for future changes
      root.children = [...inputs, labels, contents, styles];

      totalTabs++;
    });
  };
};

function generateTabInputs(count: number, prefix: string) {
  return Array.from({ length: count }, (_, i) => ({
    type: "html",
    value: `<input type="radio" id="${prefix}-${i}" name="${prefix}" ${
      i === 0 ? "checked" : ""
    }>`,
  }));
}

function generateTabLabels(tabs: ContainerDirective[], prefix: string) {
  const labels = tabs.map((tab, i) => {
    const id = `${prefix}-${i}`;
    const label = extractTabLabel(tab) || `Tab ${i + 1}`;
    return `<label for="${id}">${label}</label>`;
  });

  return {
    type: "html",
    value: `<div class="tabs-labels">${labels.join("")}</div>`,
  };
}

function extractTabLabel(tab: ContainerDirective): string | null {
  for (const child of tab.children) {
    if (child.type === "paragraph" && child.data?.directiveLabel) {
      return child.children
        .map((c) => (c.type === "text" ? c.value : ""))
        .join("")
        .trim();
    }
  }
  return null;
}

function generateTabContents(
  tabs: ContainerDirective[],
  prefix: string,
): ContainerDirective {
  return {
    type: "containerDirective",
    name: "tabs-contents",
    attributes: {},
    children: tabs.map((tab, i) => {
      const id = `${prefix}-${i}`;
      const contentChildren = tab.children.filter(
        (child) => !(child.type === "paragraph" && child.data?.directiveLabel),
      );

      return {
        type: "containerDirective",
        name: "tab-content",
        attributes: {},
        children: contentChildren,
        data: {
          hName: "div",
          hProperties: { className: ["tab-content"], id },
        },
      };
    }),
    data: {
      hName: "div",
      hProperties: { className: ["tabs-contents"] },
    },
  };
}

function generateTabStyles(count: number, prefix: string) {
  const rules = Array.from({ length: count }, (_, i) => {
    const id = `${prefix}-${i}`;
    return `
      .markdown-body #${id}:checked ~ .tabs-contents .tab-content#${id} {
        display: block;
      }
      .markdown-body #${id}:checked ~ .tabs-labels label[for="${id}"] {
        background: transparent;
        font-weight: bold;
      }
    `;
  }).join("");

  return {
    type: "html",
    value: `<style>${rules}</style>`,
  };
}

export default remarkDirectiveTabs;
