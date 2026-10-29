import * as assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { existsSync } from "node:fs";
import { mkdtemp, readFile, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { delimiter, join, resolve } from "node:path";

import { until } from "./polling.mjs";

export async function untilBrowser(
  check: () => boolean | Promise<boolean>,
): Promise<void> {
  await until(check, "browser acceptance condition", 15_000);
}

export async function withBrowser(
  run: (
    evaluate: (expression: string) => Promise<unknown>,
    navigate: (url: string) => Promise<void>,
  ) => Promise<void>,
): Promise<void> {
  const candidates = [
    process.env.FLEXIMARK_BROWSER,
    "C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe",
    "C:/Program Files/Microsoft/Edge/Application/msedge.exe",
    "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
    ...(process.env.PATH ?? "")
      .split(delimiter)
      .flatMap((directory) =>
        ["google-chrome", "chromium", "chromium-browser"].map((name) =>
          join(directory, name),
        ),
      ),
  ];
  const executable = candidates.find(
    (candidate) => candidate && existsSync(candidate),
  );
  assert.ok(
    executable,
    "Real browser acceptance requires Chrome/Edge; set FLEXIMARK_BROWSER to its executable",
  );
  const profile = await mkdtemp(join(tmpdir(), "fleximark-browser-"));
  const child = spawn(
    executable,
    [
      "--headless=new",
      "--no-sandbox",
      "--disable-gpu",
      "--no-first-run",
      "--remote-debugging-port=0",
      `--user-data-dir=${profile}`,
      "about:blank",
    ],
    { stdio: "ignore", windowsHide: true },
  );
  let launchError: Error | undefined;
  child.once("error", (error) => {
    launchError = error;
  });
  let socket: WebSocket | undefined;
  try {
    let port = "";
    await untilBrowser(async () => {
      if (launchError) throw launchError;
      assert.equal(child.exitCode, null, "browser exited before CDP was ready");
      try {
        port = (
          await readFile(join(profile, "DevToolsActivePort"), "utf8")
        ).split("\n")[0];
      } catch {
        return false;
      }
      return Boolean(port);
    });
    const pages = (await (
      await fetch(`http://127.0.0.1:${port}/json`)
    ).json()) as { type: string; webSocketDebuggerUrl: string }[];
    const page = pages.find((entry) => entry.type === "page");
    assert.ok(page);
    const connection = new WebSocket(page.webSocketDebuggerUrl);
    socket = connection;
    await new Promise<void>((resolve, reject) => {
      connection.addEventListener("open", () => resolve(), { once: true });
      connection.addEventListener(
        "error",
        () => reject(new Error("CDP connection failed")),
        { once: true },
      );
    });
    let nextId = 0;
    const request = (
      method: string,
      params: unknown,
    ): Promise<Record<string, unknown>> =>
      new Promise((resolve, reject) => {
        const id = ++nextId;
        const timeout = setTimeout(() => {
          connection.removeEventListener("message", listener);
          reject(new Error(`CDP ${method} timed out`));
        }, 10_000);
        const listener = (event: MessageEvent) => {
          const response = JSON.parse(String(event.data)) as {
            id?: number;
            error?: unknown;
            result: Record<string, unknown>;
          };
          if (response.id !== id) return;
          clearTimeout(timeout);
          connection.removeEventListener("message", listener);
          if (response.error) reject(new Error(JSON.stringify(response.error)));
          else resolve(response.result);
        };
        connection.addEventListener("message", listener);
        connection.send(JSON.stringify({ id, method, params }));
      });
    await run(
      async (expression) => {
        const result = await request("Runtime.evaluate", {
          expression,
          returnByValue: true,
          awaitPromise: true,
        });
        assert.equal(
          result.exceptionDetails,
          undefined,
          JSON.stringify(result.exceptionDetails),
        );
        return (result.result as { value?: unknown }).value;
      },
      async (url) => {
        await request("Page.navigate", { url });
      },
    );
  } finally {
    socket?.close();
    child.kill();
    if (child.exitCode === null)
      await new Promise<void>((resolve) => {
        child.once("exit", () => resolve());
        setTimeout(resolve, 5_000);
      });
    const resolvedProfile = resolve(profile);
    assert.ok(
      resolvedProfile.startsWith(resolve(tmpdir(), "fleximark-browser-")),
    );
    await rm(resolvedProfile, {
      recursive: true,
      force: true,
      maxRetries: 5,
      retryDelay: 200,
    });
  }
}
