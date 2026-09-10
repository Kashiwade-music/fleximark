import { execFileSync } from "node:child_process";
import { cp, mkdir, mkdtemp, readdir, rm } from "node:fs/promises";
import { basename, dirname, extname, join, relative, resolve } from "node:path";

const sourceRoot = resolve("src");
const outputRoot = resolve("l10n");
const temporaryRoot = await mkdtemp(join(resolve("."), ".fleximark-l10n-"));
const temporarySource = join(temporaryRoot, "src");

async function copySources(directory) {
  for (const entry of await readdir(directory, { withFileTypes: true })) {
    const source = join(directory, entry.name);
    if (entry.isDirectory()) {
      await copySources(source);
      continue;
    }
    if (![".mts", ".cts", ".ts", ".js"].includes(extname(entry.name))) {
      continue;
    }

    const relativePath = relative(sourceRoot, source);
    const targetDirectory = join(temporarySource, dirname(relativePath));
    const targetName = basename(relativePath).replace(/\.(?:mts|cts)$/, ".ts");
    await mkdir(targetDirectory, { recursive: true });
    await cp(source, join(targetDirectory, targetName));
  }
}

try {
  await copySources(sourceRoot);
  execFileSync(
    process.execPath,
    [
      resolve("node_modules/@vscode/l10n-dev/dist/cli.js"),
      "export",
      "--outDir",
      outputRoot,
      temporarySource,
    ],
    { stdio: "inherit" },
  );
} finally {
  await rm(temporaryRoot, { recursive: true, force: true });
}
