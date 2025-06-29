import { Plugin } from "unified";
import { visit } from "unist-util-visit";
import { ContainerDirective } from "mdast-util-directive";
import { Node, Parent } from "unist";
import { generateRandomString } from "../utils/rand.mjs";

const remarkDirectiveTabs: Plugin = () => {
  return (tree: Node) => {
    visit(tree, "containerDirective", (node: Node, index, parent) => {
      let rootDirective = node as ContainerDirective;

      if (rootDirective.name !== "tabs") return;

      // set root element
      rootDirective.data ??= {};
      rootDirective.data.hName = "div";
      rootDirective.data.hProperties = {
        className: ["tabs-container"],
      };

      // correct all :::tab[name] children
      let tabChildren: ContainerDirective[] = [];

      for (const child of rootDirective.children) {
        // append if child.name === "tab"
        if (child.type === "containerDirective" && child.name === "tab") {
          tabChildren.push(child);
        }
      }

      // generate tree like this:
      //   <div class="tabs-container">
      //     <input type="radio" id="tab0" name="tab" checked>
      //     <input type="radio" id="tab1" name="tab">

      //     <div class="tabs-labels">
      //       <label for="tab0">MyTab1</label>
      //       <label for="tab1">MyTab2</label>
      //     </div>

      //     <div class="tabs-contents">
      //       <div id="content1" class="tab-content">
      //         content1
      //       </div>
      //       <div id="content2" class="tab-content">
      //         content2
      //       </div>
      //     </div>
      //   </div>

      let finalChildren: any[] = [];
      const prefix = `tab-${generateRandomString(8)}`;

      // Create input elements for each tab
      const inputs = tabChildren.map((tab, i) => {
        const tabId = `${prefix}-${i}`;
        return {
          type: "html",
          value: `<input type="radio" id="${tabId}" name="${prefix}" ${
            i === 0 ? "checked" : ""
          }>`,
        };
      });
      finalChildren.push(...inputs);

      // create tabs-labels
      let tabLabelsHtml = `<div class="tabs-labels">`;
      tabChildren.forEach((tab, i) => {
        let labelText = "Tab " + (i + 1);
        for (const child of tab.children) {
          if (
            child.type === "paragraph" &&
            child.data?.directiveLabel === true
          ) {
            // title node
            labelText = child.children
              .map((c) => (c.type === "text" ? c.value : ""))
              .join("");
            break;
          }
        }

        const tabId = `${prefix}-${i}`;
        tabLabelsHtml += `<label for="${tabId}">${labelText}</label>`;
      });
      tabLabelsHtml += `</div>`;
      finalChildren.push({
        type: "html",
        value: tabLabelsHtml,
      });

      // create tabs-contents
      let tabsContents: ContainerDirective = {
        type: "containerDirective",
        name: "tabs-contents",
        attributes: {},
        children: [],
        data: {
          hName: "div",
          hProperties: {
            className: ["tabs-contents"],
          },
        },
      };
      tabChildren.forEach((tab, i) => {
        const tabId = `${prefix}-${i}`;

        let newTab: ContainerDirective = {
          type: "containerDirective",
          name: "tab-content",
          attributes: {},
          children: tab.children.filter((child) => {
            // filter out the paragraph that is the label
            return !(
              child.type === "paragraph" && child.data?.directiveLabel === true
            );
          }),
          data: {
            hName: "div",
            hProperties: {
              className: ["tab-content"],
              id: tabId,
            },
          },
        };

        tabsContents.children.push(newTab);
      });

      finalChildren.push(tabsContents);

      // generate CSS
      //
      // /* タブ1 */
      // #tab-KcEyncZ9-0:checked ~ .tabs-contents #tab-KcEyncZ9-0 {
      //   display: block;
      // }
      // #tab-KcEyncZ9-0:checked ~ .tabs-labels label[for="tab-KcEyncZ9-0"] {
      //   background: #fff;
      //   font-weight: bold;
      // }

      // /* タブ2 */
      // #tab-KcEyncZ9-1:checked ~ .tabs-contents #tab-KcEyncZ9-1 {
      //   display: block;
      // }
      // #tab-KcEyncZ9-1:checked ~ .tabs-labels label[for="tab-KcEyncZ9-1"] {
      //   background: #fff;
      //   font-weight: bold;
      // }

      const cssRules =
        "<style>" +
        tabChildren
          .map((_, i) => {
            const tabId = `${prefix}-${i}`;
            return `.markdown-body #${tabId}:checked~.tabs-contents .tab-content#${tabId}{display:block}.markdown-body #${tabId}:checked~.tabs-labels label[for="${tabId}"]{background:transparent;font-weight:bold}`;
          })
          .join("") +
        "</style>";

      const styleElement = {
        type: "html",
        value: cssRules,
      };

      finalChildren.push(styleElement);

      // replace rootDirective's children with finalChildren
      rootDirective.children = finalChildren;

      console.log(rootDirective);

      // print as json
      console.log(JSON.stringify(rootDirective, null, 2));
    });
  };
};

export default remarkDirectiveTabs;
