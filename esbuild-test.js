#!/usr/bin/env node
import console from "console";
import esbuild from "esbuild";
import fs from "fs/promises";
import path from "path";
import process from "process";
import { fileURLToPath } from "url";

const __dirname = path.dirname(fileURLToPath(import.meta.url));

const watch = process.argv.includes("--watch");
const cleanOnly = process.argv.includes("--clean");
const testOutputDir = path.join(__dirname, "out", "test");

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
  await fs.rm(testOutputDir, { force: true, recursive: true });
  if (cleanOnly) {
    return;
  }

  const ctx = await esbuild.context({
    entryPoints: ["test/extension.test.mts"],
    bundle: true,
    platform: "node",
    format: "cjs",
    sourcemap: true,
    outfile: "out/test/extension.test.cjs",
    external: ["vscode"],
    logLevel: "silent",
    plugins: [esbuildProblemMatcherPlugin],
    loader: {
      ".css": "text",
    },
  });

  if (watch) {
    await ctx.watch();
  } else {
    try {
      await ctx.rebuild();
    } finally {
      await ctx.dispose();
    }
  }
}

main().catch((e) => {
  console.error(e);
  process.exit(1);
});
