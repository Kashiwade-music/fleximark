#!/usr/bin/env node
import console from "console";
import esbuild from "esbuild";
import fs from "fs/promises";
import path from "path";
import process from "process";
import { fileURLToPath } from "url";

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);

const production = process.argv.includes("--production");
const watch = process.argv.includes("--watch");

/**
 * @type {import('esbuild').Plugin}
 */
const esbuildProblemMatcherPlugin = {
  name: "esbuild-problem-matcher",

  setup(build) {
    build.onStart(() => {
      console.log("[watch] build started");
    });
    build.onEnd((result) => {
      result.errors.forEach(({ text, location }) => {
        console.error(`✘ [ERROR] ${text}`);
        console.error(
          `    ${location.file}:${location.line}:${location.column}:`,
        );
      });
      console.log("[watch] build finished");
    });
  },
};

async function main() {
  // Extension build
  const extensionCtx = await esbuild.context({
    entryPoints: ["src/extension.mts"],
    bundle: true,
    format: "cjs",
    minify: production,
    sourcemap: !production,
    sourcesContent: false,
    platform: "node",
    outfile: "dist/extension.cjs",
    external: ["vscode"],
    logLevel: "silent",
    plugins: [
      /* add to the end of plugins array */
      esbuildProblemMatcherPlugin,
    ],
    loader: {
      ".css": "text",
    },
    define: {
      __DEV__: production ? "false" : "true",
    },
  });

  // Script for Webview build
  const mediaCtx = await esbuild.context({
    entryPoints: [
      "media/client/browser.mts",
      "media/client/file.mts",
      "media/client/webview.mts",
    ],
    bundle: true,
    format: "iife",
    platform: "browser",
    minify: production,
    sourcemap: !production,
    outdir: "dist/media",
    logLevel: "silent",
    plugins: [esbuildProblemMatcherPlugin],
  });

  const copyAssets = async () => {
    try {
      const mediaDir = path.join(__dirname, "dist", "media");
      await fs.mkdir(mediaDir, { recursive: true });

      const assets = [
        {
          src: path.join(__dirname, "media", "workspaceSettingsJsonTemplate"),
          dest: path.join(mediaDir, "workspaceSettingsJsonTemplate"),
        },
        {
          src: path.join(__dirname, "node_modules", "abcjs", "abcjs-audio.css"),
          dest: path.join(mediaDir, "abcjs-audio.css"),
        },
        {
          src: path.join(
            __dirname,
            "node_modules",
            "katex",
            "dist",
            "katex.min.css",
          ),
          dest: path.join(mediaDir, "katex.min.css"),
        },
        {
          src: path.join(__dirname, "node_modules", "katex", "dist", "fonts"),
          dest: path.join(mediaDir, "fonts"),
        },
      ];

      for (const { src, dest } of assets) {
        const stat = await fs.stat(src);
        if (stat.isDirectory()) {
          await fs.mkdir(dest, { recursive: true });
          await fs.cp(src, dest, { recursive: true });
        } else if (stat.isFile()) {
          await fs.copyFile(src, dest);
        } else {
          console.warn(`Skipping unknown type: ${src}`);
        }
      }

      console.log("Assets successfully copied to dist/media");
    } catch (error) {
      console.error("Failed to copy assets:", error);
    }
  };

  if (watch) {
    await extensionCtx.watch();
    await mediaCtx.watch();
    await copyAssets();
  } else {
    await extensionCtx.rebuild();
    await extensionCtx.dispose();
    await mediaCtx.rebuild();
    await mediaCtx.dispose();
    await copyAssets();
  }
}

main().catch((e) => {
  console.error(e);
  process.exit(1);
});
