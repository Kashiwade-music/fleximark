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
const cleanOnly = process.argv.includes("--clean");
const distDir = path.join(__dirname, "dist");

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
  await fs.rm(distDir, { force: true, recursive: true });
  if (cleanOnly) {
    return;
  }

  // Thin VS Code adapter. The legacy TypeScript runtime is not shipped.
  const extensionCtx = await esbuild.context({
    entryPoints: ["adapters/vscode/src/extension.mts"],
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

  // Both preview targets use the same client-side renderers and local assets.
  const mediaCtx = await esbuild.context({
    entryPoints: [
      "web/preview-client/vscode-host.mts",
      "web/preview-client/browser-host.mts",
    ],
    bundle: true,
    format: "iife",
    platform: "browser",
    minify: production,
    sourcemap: !production,
    outdir: "dist/web/preview-client",
    logLevel: "silent",
    plugins: [esbuildProblemMatcherPlugin],
  });

  if (watch) {
    await extensionCtx.watch();
    await mediaCtx.watch();
  } else {
    try {
      await extensionCtx.rebuild();
      await mediaCtx.rebuild();
    } finally {
      await Promise.all([extensionCtx.dispose(), mediaCtx.dispose()]);
    }
  }
}

main().catch((e) => {
  console.error(e);
  process.exit(1);
});
