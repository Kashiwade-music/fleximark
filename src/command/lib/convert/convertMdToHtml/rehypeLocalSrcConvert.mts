import { Express } from "express";
import * as fs from "fs";
import * as fsPromises from "fs/promises";
import { Element, Root } from "hast";
import * as path from "path";
import { Plugin } from "unified";
import { visit } from "unist-util-visit";
import * as vscode from "vscode";

interface WebviewArgs {
  convertType: "webview";
  markdownAbsPath: string;
  webview: vscode.Webview;
}

interface BrowserArgs {
  convertType: "browser";
  markdownAbsPath: string;
  app: Express;
}

interface FileArgs {
  convertType: "file";
  markdownAbsPath: string;
  distDir: string;
}

export type RehypeLocalSrcConvertArgs = WebviewArgs | BrowserArgs | FileArgs;

const rehypeLocalSrcConvert: Plugin<[RehypeLocalSrcConvertArgs], Root> = (
  args,
) => {
  return (hast: Root) => {
    switch (args.convertType) {
      case "webview":
        convertLocalSrcForWebview(hast, args.markdownAbsPath, args.webview);
        break;
      case "browser":
        convertLocalSrcForBrowser(hast, args.markdownAbsPath, args.app);
        break;
      case "file":
        convertLocalSrcForFile(hast, args.distDir, args.markdownAbsPath);
        break;
      default:
        return;
    }
  };
};

export default rehypeLocalSrcConvert;

const tagAttrMap: Record<string, string> = {
  img: "src",
  video: "src",
  audio: "src",
};

function convertLocalSrcForWebview(
  hast: Root,
  markdownAbsPath: string,
  webview: vscode.Webview,
): Root {
  visit(hast, "element", (node: Element) => {
    const tag = node.tagName;
    const attr = tagAttrMap[tag];
    if (!attr || !node.properties) return;

    const attrValue = node.properties[attr];
    if (typeof attrValue !== "string") return;

    const src = attrValue;
    try {
      if (src.startsWith("http") || src.startsWith("data:")) {
        return;
      }

      const localPath = decodeURIComponent(
        path.isAbsolute(src)
          ? src
          : path.join(path.dirname(markdownAbsPath), src),
      );

      const uri = webview.asWebviewUri(vscode.Uri.file(localPath));
      node.properties.src = uri.toString(true);
    } catch (err) {
      console.error(`Failed to rewrite img src for: ${src}`, err);
    }
  });

  return hast;
}

const registeredStaticFiles = new Set<string>();

function toUrlFromLocalPath(localPath: string): string {
  const parsedPath = path
    .parse(localPath)
    .dir // 'C:/hoge'
    .split(path.sep) // ['C:', 'hoge']
    .concat(path.parse(localPath).base) // ['C:', 'hoge', 'puni']
    .map((part) => encodeURIComponent(part.replace(":", "").toLowerCase())) // ['c', 'hoge', 'puni']
    .join("/"); // 'c/hoge/puni'

  return "/static/" + parsedPath;
}

function convertLocalSrcForBrowser(
  hast: Root,
  markdownAbsPath: string,
  app: Express,
): Root {
  visit(hast, "element", (node: Element) => {
    const tag = node.tagName;
    const attr = tagAttrMap[tag];
    if (!attr || !node.properties) return;

    const attrValue = node.properties[attr];
    if (typeof attrValue !== "string") return;

    const src = attrValue;
    try {
      if (src.startsWith("http") || src.startsWith("data:")) {
        return;
      }

      const localPath = decodeURIComponent(
        path.isAbsolute(src)
          ? src
          : path.join(path.dirname(markdownAbsPath), src),
      );

      const urlPath = toUrlFromLocalPath(path.normalize(localPath));
      node.properties.src = urlPath;

      if (!registeredStaticFiles.has(localPath)) {
        app.get(urlPath, (req, res) => {
          res.sendFile(localPath);
        });
        registeredStaticFiles.add(localPath);
      }
    } catch (err) {
      console.error(`Failed to rewrite img src for: ${src}`, err);
    }
  });

  return hast;
}

async function convertLocalSrcForFile(
  hast: Root,
  distDir: string,
  markdownAbsPath: string,
): Promise<Root> {
  const copiedFiles = new Set<string>();

  const copyPromises: Promise<void>[] = [];

  visit(hast, "element", (node: Element) => {
    const tag = node.tagName;
    const attr = tagAttrMap[tag];
    if (!attr || !node.properties) return;

    const attrValue = node.properties[attr];
    if (typeof attrValue !== "string") return;

    const src = attrValue;
    try {
      if (src.startsWith("http") || src.startsWith("data:")) {
        return;
      }

      const localPath = decodeURIComponent(
        path.isAbsolute(src)
          ? src
          : path.join(path.dirname(markdownAbsPath), src),
      );

      const absoluteLocalPath = path.resolve(localPath);

      const fileName = path.basename(localPath);
      const destPath = path.resolve(distDir, fileName);

      if (!copiedFiles.has(absoluteLocalPath)) {
        const destDir = path.dirname(destPath);
        if (!fs.existsSync(destDir)) {
          fs.mkdirSync(destDir, { recursive: true });
        }

        const copyPromise = fsPromises.copyFile(absoluteLocalPath, destPath);
        copyPromises.push(copyPromise);
        copiedFiles.add(absoluteLocalPath);
      }

      node.properties[attr] = `./${fileName}`;
    } catch (err) {
      console.error(`Failed to rewrite src for: ${src}`, err);
    }
  });

  await Promise.all(copyPromises);

  return hast;
}
