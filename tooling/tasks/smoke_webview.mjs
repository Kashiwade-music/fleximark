import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { mkdir, readFile, writeFile } from "node:fs/promises";
import { join, resolve } from "node:path";
import process from "node:process";

// Inspect the installed product's actual Webview, including VS Code's API shim
// and postMessage delivery. Extension-host state alone cannot detect a blank DOM.
const [executable, scratch, extensions] = process.argv.slice(2);
assert.ok(executable && scratch && extensions);
const profile = join(scratch, "webview-profile");
const workspace = join(scratch, "webview-workspace");
await mkdir(join(profile, "User"), { recursive: true });
await mkdir(workspace, { recursive: true });
await writeFile(
  join(profile, "User", "settings.json"),
  JSON.stringify({
    "fleximark.autoOpenPreview": true,
    "fleximark.previewTarget": "embeddedHtml",
    "security.workspace.trust.enabled": false,
    "workbench.startupEditor": "none",
    "window.restoreWindows": "none",
  }),
);
const source = await readFile(resolve("docs/DEVELOPMENT.md"), "utf8");
const heading = /^# (.+)$/m.exec(source)?.[1];
assert.ok(heading, "development document must have a heading");
const document = join(workspace, "DEVELOPMENT.md");
await writeFile(document, source);
const env = { ...process.env };
delete env.ELECTRON_RUN_AS_NODE;
const child = spawn(
  executable,
  [
    "--remote-debugging-port=0",
    "--new-window",
    "--skip-welcome",
    "--skip-release-notes",
    "--disable-workspace-trust",
    "--disable-updates",
    "--no-sandbox",
    "--disable-gpu-sandbox",
    `--user-data-dir=${profile}`,
    `--extensions-dir=${extensions}`,
    workspace,
    document,
  ],
  { env, windowsHide: true, stdio: ["ignore", "ignore", "pipe"] },
);
let diagnostics = "";
let launchError;
child.on("error", (error) => {
  launchError = error;
});
child.stderr.on("data", (value) => {
  diagnostics = (diagnostics + value).slice(-8000);
});
const sockets = [];
const sleep = (milliseconds) =>
  new Promise((resolve) => globalThis.setTimeout(resolve, milliseconds));

async function until(read, label) {
  const deadline = Date.now() + 30_000;
  while (Date.now() < deadline) {
    if (launchError) throw launchError;
    assert.equal(child.exitCode, null, `VS Code exited: ${diagnostics}`);
    const value = await read();
    if (value) return value;
    await sleep(100);
  }
  throw new Error(`Timed out waiting for ${label}. ${diagnostics}`);
}

async function connect(url) {
  const socket = new globalThis.WebSocket(url);
  sockets.push(socket);
  await new Promise((resolve, reject) => {
    socket.addEventListener("open", resolve, { once: true });
    socket.addEventListener("error", reject, { once: true });
  });
  const contexts = new Set();
  const pending = new Map();
  let nextId = 0;
  socket.addEventListener("message", (event) => {
    const message = JSON.parse(String(event.data));
    if (message.method === "Runtime.executionContextCreated")
      contexts.add(message.params.context.id);
    if (message.method === "Runtime.executionContextDestroyed")
      contexts.delete(message.params.executionContextId);
    if (message.method === "Runtime.executionContextsCleared") contexts.clear();
    if (!message.id) return;
    const request = pending.get(message.id);
    if (!request) return;
    pending.delete(message.id);
    globalThis.clearTimeout(request.timeout);
    if (message.error) request.reject(new Error(message.error.message));
    else request.resolve(message.result);
  });
  const request = (method, params = {}) =>
    new Promise((resolve, reject) => {
      const id = ++nextId;
      const timeout = globalThis.setTimeout(() => {
        pending.delete(id);
        reject(new Error(`CDP ${method} timed out`));
      }, 5000);
      pending.set(id, { resolve, reject, timeout });
      socket.send(JSON.stringify({ id, method, params }));
    });
  await request("Runtime.enable");
  return { contexts, request };
}

let mainWindow;
try {
  const port = await until(async () => {
    try {
      return (
        await readFile(join(profile, "DevToolsActivePort"), "utf8")
      ).split("\n")[0];
    } catch {
      return undefined;
    }
  }, "VS Code debugging endpoint");
  const endpoint = `http://127.0.0.1:${port}`;
  const workbench = await until(async () => {
    const targets = await (
      await globalThis.fetch(`${endpoint}/json/list`)
    ).json();
    return targets.find((target) => target.type === "page");
  }, "VS Code workbench target");
  mainWindow = await connect(workbench.webSocketDebuggerUrl);
  const target = await until(async () => {
    const targets = await (
      await globalThis.fetch(`${endpoint}/json/list`)
    ).json();
    return targets.find((target) =>
      /[?&]extensionId=kashiwade\.fleximark(?:&|$)/i.test(target.url),
    );
  }, "installed FlexiMark Webview");
  const webview = await connect(target.webSocketDebuggerUrl);
  const rendered = async (expected) => {
    for (const contextId of webview.contexts) {
      const result = await webview.request("Runtime.evaluate", {
        contextId,
        expression: `document.querySelector('#preview h1')?.textContent === ${JSON.stringify(heading)} && document.querySelector('#preview')?.textContent.includes(${JSON.stringify(expected)})`,
        returnByValue: true,
      });
      if (result.result?.value === true) return true;
    }
    return false;
  };
  await until(
    () => rendered("mise run package vscode"),
    "DEVELOPMENT.md content in the installed Webview DOM",
  );
  const marker = "Packaged Webview live update verified";
  await writeFile(document, `${source}\n\n${marker}\n`);
  await until(
    () => rendered(marker),
    "live document update in the Webview DOM",
  );
  globalThis.console.log(
    "Installed VSIX rendered DEVELOPMENT.md and a live edit in its real Webview.",
  );
} finally {
  try {
    await mainWindow?.request("Page.close");
  } catch {
    // The window may already have closed after a failed launch.
  }
  if (child.exitCode === null)
    await Promise.race([
      new Promise((resolve) => child.once("exit", resolve)),
      sleep(5000),
    ]);
  for (const socket of sockets) socket.close();
  if (child.exitCode === null) child.kill();
  if (child.exitCode === null)
    await Promise.race([
      new Promise((resolve) => child.once("exit", resolve)),
      sleep(5000),
    ]);
  await sleep(500);
}
