import { spawn } from "node:child_process";
import path from "node:path";
import process from "node:process";
import { fileURLToPath } from "node:url";

const root = path.dirname(path.dirname(fileURLToPath(import.meta.url)));
const node = process.execPath;

function run(command, args, options = {}) {
  return new Promise((resolve, reject) => {
    const child = spawn(command, args, {
      cwd: root,
      env: process.env,
      stdio: "inherit",
      ...options,
    });
    child.once("error", reject);
    child.once("exit", (code, signal) => {
      if (code === 0) resolve();
      else
        reject(
          new Error(
            `${path.basename(command)} failed${signal ? ` with ${signal}` : ` with exit code ${code}`}`,
          ),
        );
    });
  });
}

const runNode = (script, ...args) => run(node, [script, ...args]);
const runCargo = (...args) => run("cargo", args);

async function clean() {
  await runNode("esbuild.js", "--clean");
  await runNode("esbuild-test.js", "--clean");
}

async function build() {
  await runNode("scripts/build-browser-client.mjs");
  await runCargo("build", "--release", "-p", "fleximarkd");
  await runNode("scripts/stage-daemon.mjs");
  await runNode("scripts/create-release-manifest.mjs");
  await runNode("esbuild.js", "--production");
}

async function dev() {
  const children = [
    spawn(node, ["esbuild.js", "--watch"], {
      cwd: root,
      env: process.env,
      stdio: "inherit",
    }),
    spawn(
      node,
      [
        "node_modules/typescript/bin/tsc",
        "--noEmit",
        "--watch",
        "--project",
        "tsconfig.json",
      ],
      { cwd: root, env: process.env, stdio: "inherit" },
    ),
  ];
  let stopping = false;
  const stop = () => {
    if (stopping) return;
    stopping = true;
    for (const child of children) child.kill();
  };
  process.once("SIGINT", stop);
  process.once("SIGTERM", stop);
  await Promise.race(
    children.map(
      (child) =>
        new Promise((resolve, reject) => {
          child.once("error", reject);
          child.once("exit", (code, signal) =>
            code === 0 || stopping
              ? resolve()
              : reject(
                  new Error(
                    `development watcher failed${signal ? ` with ${signal}` : ` with exit code ${code}`}`,
                  ),
                ),
          );
        }),
    ),
  );
  stop();
}

async function compileTests() {
  await runNode("esbuild-test.js");
}

async function integrationTest() {
  await compileTests();
  await build();
  await runNode("node_modules/@vscode/test-cli/out/bin.mjs");
}

async function checkLocalization() {
  await runNode("scripts/l10n-export.mjs");
  await run("git", ["diff", "--exit-code", "--", "l10n"]);
}

async function verify() {
  await runNode("scripts/verify-architecture.mjs");
  await runNode("node_modules/typescript/bin/tsc", "--noEmit");
  await runNode(
    "node_modules/eslint/bin/eslint.js",
    "adapters",
    "web",
    "test",
    "scripts",
  );
  await checkLocalization();
  await build();
  await compileTests();
  await runNode("node_modules/@vscode/vsce/vsce", "ls", "--no-dependencies");
  await runCargo("fmt", "--all", "--", "--check");
  await runCargo("test", "--workspace", "--all-targets");
  await runCargo(
    "clippy",
    "--workspace",
    "--all-targets",
    "--",
    "-D",
    "warnings",
  );
  await runNode("scripts/check-performance-budgets.mjs");
  await runNode("node_modules/@vscode/test-cli/out/bin.mjs");
}

async function packageVsix(args) {
  await verify();
  await runNode(
    "node_modules/@vscode/vsce/vsce",
    "package",
    "--no-dependencies",
    ...args,
  );
}

const [task, ...args] = process.argv.slice(2);
const tasks = {
  build,
  clean,
  dev,
  package: () => packageVsix(args),
  test: integrationTest,
  verify,
};

if (!Object.hasOwn(tasks, task)) {
  throw new Error(`unknown task: ${task ?? "(missing)"}`);
}

await tasks[task]();
