import * as fs from "fs";
import { Root } from "mdast";
import { ContainerDirective } from "mdast-util-directive";
import * as path from "path";
import remarkDirective from "remark-directive";
import remarkFrontmatter from "remark-frontmatter";
import remarkGfm from "remark-gfm";
import remarkMath from "remark-math";
import remarkParse from "remark-parse";
import remarkStringify from "remark-stringify";
import { unified } from "unified";
import { visit } from "unist-util-visit";

interface Dictionary {
  absPath: string;
  admonitions: ContainerDirective[];
}

function process(categoryRootAbsDirPath: string) {
  const dictionaries = getAdmonitionDictionaries(
    categoryRootAbsDirPath,
    categoryRootAbsDirPath,
  );

  const root: Root = {
    type: "root",
    children: [
      {
        type: "heading",
        depth: 1,
        children: [{ type: "text", value: "Admonitions" }],
      },
    ],
  };

  for (const dict of dictionaries) {
    const fileName = path.basename(dict.absPath);
    const relativePath = path.relative(categoryRootAbsDirPath, dict.absPath);

    root.children.push({
      type: "heading",
      depth: 2,
      children: [
        {
          type: "link",
          url: relativePath.replace(/\\/g, "/"),
          children: [{ type: "text", value: fileName }],
        },
      ],
    });

    root.children.push(...dict.admonitions);
  }

  const file = unified()
    .use(remarkDirective)
    .use(remarkStringify)
    .stringify(root);

  return file;
}

export default process;

function getAdmonitionDictionaries(
  targetDirPath: string,
  categoryRootAbsDirPath: string,
): Dictionary[] {
  const entries = fs.readdirSync(targetDirPath, { withFileTypes: true });
  const dictionaries: Dictionary[] = [];

  for (const entry of entries) {
    const fullPath = path.join(targetDirPath, entry.name);

    if (entry.isDirectory()) {
      dictionaries.push(
        ...getAdmonitionDictionaries(fullPath, categoryRootAbsDirPath),
      );
    } else if (entry.isFile() && fullPath.endsWith(".md")) {
      const content = fs.readFileSync(fullPath, "utf-8");
      const admonitions = getAdmonitionDirectives(
        content,
        fullPath,
        categoryRootAbsDirPath,
      );
      if (admonitions.length !== 0) {
        dictionaries.push({
          absPath: fullPath,
          admonitions,
        });
      }
    }
  }

  return dictionaries;
}

const ALLOWED_TYPES = ["info", "tip", "warning", "danger"] as const;
type AllowedType = (typeof ALLOWED_TYPES)[number];

function getAdmonitionDirectives(
  content: string,
  sourceAbsPath: string,
  targetAbsDirPath: string,
): ContainerDirective[] {
  const remarkProcessor = unified()
    .use(remarkParse)
    .use(remarkFrontmatter)
    .use(remarkGfm)
    .use(remarkMath)
    .use(remarkDirective);

  const tree = remarkProcessor.parse(content);
  const directives: ContainerDirective[] = [];

  visit(tree, "containerDirective", (node: ContainerDirective) => {
    if (ALLOWED_TYPES.includes(node.name as AllowedType)) {
      // パス修正のためにリンクや画像を訪問
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      visit(node, (child: any) => {
        if ((child.type === "image" || child.type === "link") && child.url) {
          const url = child.url;

          if (
            !url.startsWith("http://") &&
            !url.startsWith("https://") &&
            !url.startsWith("mailto:") &&
            !path.isAbsolute(url)
          ) {
            const resolvedPath = path.resolve(path.dirname(sourceAbsPath), url);
            const newRelativePath = path.relative(
              targetAbsDirPath,
              resolvedPath,
            );
            child.url = newRelativePath.replace(/\\/g, "/");
          }
        }
      });

      directives.push(node);
    }
  });

  return directives;
}
