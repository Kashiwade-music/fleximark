import esbuild from "esbuild";
import console from "node:console";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = path.dirname(path.dirname(fileURLToPath(import.meta.url)));
const output = path.join(root, "web", "preview-client", "browser-host.js");
await esbuild.build({
  absWorkingDir: root,
  entryPoints: ["web/preview-client/browser-host.mts"],
  bundle: true,
  format: "iife",
  platform: "browser",
  minify: true,
  outfile: output,
  logLevel: "silent",
});
if (!fs.statSync(output).size)
  throw new Error("browser client bundle is empty");
console.log(path.relative(root, output));
