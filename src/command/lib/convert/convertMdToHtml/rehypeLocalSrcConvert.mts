import { Express } from "express";
import * as fsPromises from "fs/promises";
import { Element, Root, RootContent } from "hast";
import * as path from "path";
import { Plugin } from "unified";
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
  if (args.convertType === "file") {
    const { distDir, markdownAbsPath } = args as FileArgs;
    return async (hast: Root) => {
      await convertLocalSrcForFile(hast, distDir, markdownAbsPath);
    };
  }

  if (args.convertType === "browser") {
    const { app, markdownAbsPath } = args as BrowserArgs;
    return (hast: Root) => {
      convertLocalSrcForBrowser(hast, markdownAbsPath, app);
    };
  }

  // webview
  const { webview, markdownAbsPath } = args as WebviewArgs;
  return (hast: Root) => {
    convertLocalSrcForWebview(hast, markdownAbsPath, webview);
  };
};

export default rehypeLocalSrcConvert;

// ---------------------------------------------
// Utilities
// ---------------------------------------------

const TAG_ATTR = new Map<string, "src">([
  ["img", "src"],
  ["video", "src"],
  ["audio", "src"],
]);

const EXTERNAL_RE = /^(?:https?:|data:)/i;

function forEachElement(root: Root, fn: (node: Element) => void) {
  // 確実に最小の配列確保回数に抑える
  const stack: RootContent[] = (root.children ?? []).slice();

  while (stack.length) {
    const cur = stack.pop();
    if (!cur) continue;

    if (cur.type === "element") fn(cur as Element);

    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    const ch = (cur as any).children as RootContent[] | undefined;
    if (ch && ch.length) {
      for (let i = ch.length - 1; i >= 0; i--) stack.push(ch[i]);
    }
  }
}

/** デコードは必要な時だけ。join/resolve の前に不要な decode を避ける */
function maybeDecode(p: string): string {
  return p.indexOf("%") >= 0 ? decodeURIComponent(p) : p;
}

/** 汎用: ローカルパスへ解決（base は markdown のディレクトリ） */
function resolveLocal(src: string, baseDir: string): string {
  const raw = path.isAbsolute(src) ? src : path.join(baseDir, src);
  return path.resolve(maybeDecode(raw));
}

// ---------------------------------------------
// Webview
// ---------------------------------------------

function convertLocalSrcForWebview(
  hast: Root,
  markdownAbsPath: string,
  webview: vscode.Webview,
): Root {
  const baseDir = path.dirname(markdownAbsPath);

  forEachElement(hast, (node) => {
    const attrName = TAG_ATTR.get(node.tagName);
    if (!attrName || !node.properties) return;

    const val = (node.properties as Record<string, unknown>)[attrName];
    if (typeof val !== "string" || EXTERNAL_RE.test(val)) return;

    try {
      const localPath = resolveLocal(val, baseDir);
      const uri = webview.asWebviewUri(vscode.Uri.file(localPath));
      (node.properties as Record<string, unknown>)[attrName] =
        uri.toString(true);
    } catch (err) {
      console.error(`Failed to rewrite src for: ${val}`, err);
    }
  });

  return hast;
}

// ---------------------------------------------
// Browser
// ---------------------------------------------

const urlToLocal = new Map<string, string>();

function hasStaticRoute(app: Express): boolean {
  if (!app._router) return false;
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  return app._router.stack.some((layer: any) => {
    return (
      layer.route && layer.route.path === "/static/*" && layer.route.methods.get
    );
  });
}

function toUrlFromLocalPath(localPath: string): string {
  // 例: C:\foo\bar\baz.png -> /static/c/foo/bar/baz.png
  const parsed = path.parse(localPath);
  const parts = parsed.dir
    .split(path.sep)
    .concat(parsed.base)
    .map((p) => encodeURIComponent(p.replace(":", "").toLowerCase()));
  return "/static/" + parts.join("/");
}

function ensureBrowserRoute(app: Express) {
  if (hasStaticRoute(app)) return;
  // 1本だけの動的ルート。登録済みMapに無ければ 404。
  app.get("/static/*path", (req, res) => {
    const p = req.path; // 例: /static/c/foo/bar.png
    const local = urlToLocal.get(p);
    if (!local) {
      res.status(404).end();
      return;
    }
    res.sendFile(local);
  });
}

function convertLocalSrcForBrowser(
  hast: Root,
  markdownAbsPath: string,
  app: Express,
): Root {
  ensureBrowserRoute(app);
  const baseDir = path.dirname(markdownAbsPath);

  forEachElement(hast, (node) => {
    const attrName = TAG_ATTR.get(node.tagName);
    if (!attrName || !node.properties) return;

    const val = (node.properties as Record<string, unknown>)[attrName];
    if (typeof val !== "string" || EXTERNAL_RE.test(val)) return;

    try {
      const localPath = resolveLocal(val, baseDir);
      const norm = path.normalize(localPath);
      const urlPath = toUrlFromLocalPath(norm);
      (node.properties as Record<string, unknown>)[attrName] = urlPath;

      if (!urlToLocal.has(urlPath)) urlToLocal.set(urlPath, norm);
    } catch (err) {
      console.error(`Failed to rewrite src for: ${val}`, err);
    }
  });

  return hast;
}

// ---------------------------------------------
// File
// ---------------------------------------------

async function convertLocalSrcForFile(
  hast: Root,
  distDir: string,
  markdownAbsPath: string,
): Promise<Root> {
  const baseDir = path.dirname(markdownAbsPath);

  await fsPromises.mkdir(distDir, { recursive: true });

  const toCopy = new Map<string, string>(); // srcAbs -> destAbs
  const usedNames = new Set<string>();

  forEachElement(hast, (node) => {
    const attrName = TAG_ATTR.get(node.tagName);
    if (!attrName || !node.properties) return;

    const val = (node.properties as Record<string, unknown>)[attrName];
    if (typeof val !== "string" || EXTERNAL_RE.test(val)) return;

    try {
      const srcAbs = resolveLocal(val, baseDir);

      // 同名衝突を避けるため、軽量な一意化（basename(1), basename(2), ...）
      const base = path.basename(srcAbs);
      let fileName = base;
      if (usedNames.has(fileName)) {
        const ext = path.extname(base);
        const stem = base.slice(0, -ext.length);
        let i = 2;
        while (usedNames.has(`${stem}(${i})${ext}`)) i++;
        fileName = `${stem}(${i})${ext}`;
      }
      usedNames.add(fileName);

      const destAbs = path.resolve(distDir, fileName);
      toCopy.set(srcAbs, destAbs);

      (node.properties as Record<string, unknown>)[attrName] = `./${fileName}`;
    } catch (err) {
      console.error(`Failed to rewrite src for: ${val}`, err);
    }
  });

  await Promise.all(
    Array.from(toCopy.entries()).map(async ([src, dest]) => {
      try {
        await fsPromises.copyFile(src, dest);
      } catch (e) {
        console.error(`Failed to copy: ${src} -> ${dest}`, e);
      }
    }),
  );

  return hast;
}
