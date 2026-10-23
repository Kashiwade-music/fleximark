import * as assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import * as path from "node:path";

import { validateContractSchema } from "../../contracts/typescript/contract-validator.mjs";
import {
  PROTOCOL_VERSION,
  clientNotificationValidators,
  customRequestParamsValidators,
  customRequestResultValidators,
  hasContractShape,
  isJsonRpcMessageEnvelope,
  isJsonRpcResponseEnvelope,
  isRenderFrame,
  serverNotificationValidators,
} from "../../contracts/typescript/protocol.mjs";

export const suiteName = "TypeScript protocol contract";

export function suite(): void {
  test("shared schema fixtures use the production validator", () => {
    const read = (name: string) => JSON.parse(readFileSync(name, "utf8"));
    const schema = read("contracts/schemas/protocol.schema.json");
    const check = (definition: string, value: unknown) =>
      validateContractSchema(schema.$defs, schema.$defs[definition], value);
    const fixture = read("tests/fixtures/protocol/contract.json");
    const results = schema.$defs.methodResults.properties;
    assert.deepEqual(
      Object.keys(results).sort(),
      fixture.methods.map((item: { method: string }) => item.method).sort(),
    );
    for (const item of fixture.methods) {
      const envelope = {
        jsonrpc: "2.0",
        method: item.method,
        params: item.params,
        ...(item.kind === "request" ? { id: 7 } : {}),
      };
      assert.equal(
        check(
          item.kind === "request" ? "customRequest" : "customNotification",
          envelope,
        ),
        true,
        item.method,
      );
      if (results[item.method])
        assert.equal(
          validateContractSchema(
            schema.$defs,
            results[item.method],
            item.result,
          ),
          true,
          item.method,
        );
      if (item.kind === "request")
        assert.equal(
          check("response", { jsonrpc: "2.0", id: 7, result: item.result }),
          true,
        );
      else assert.equal(item.result, null);
    }
    const server = read("tests/fixtures/protocol/bidirectional-server.json");
    assert.deepEqual(
      server.notifications
        .map((item: { method: string }) => item.method)
        .sort(),
      fixture.methods
        .filter(
          (item: { direction: string }) => item.direction === "bidirectional",
        )
        .map((item: { method: string }) => item.method)
        .sort(),
    );
    for (const item of server.notifications)
      assert.equal(
        check("serverToClientCustomNotification", {
          jsonrpc: "2.0",
          method: item.method,
          params: item.params,
        }),
        true,
      );
    for (const item of read("tests/fixtures/protocol/invalid.json").cases)
      assert.equal(check(item.definition, item.value), false, item.name);
    for (const item of read("tests/fixtures/protocol/optional-present.json")
      .cases)
      assert.equal(check(item.definition, item.value), true, item.name);
    for (const event of read("tests/fixtures/protocol/render-navigation.json")
      .events)
      assert.equal(check("renderNavigationEvent", event), true);
  });
  test("schema combinators, siblings, JSON equality and depth stay exact", () => {
    const check = (schema: unknown, value: unknown) =>
      validateContractSchema({}, schema, value);
    assert.equal(
      check({ oneOf: [{ type: "integer" }, { type: "number" }] }, 1),
      false,
    );
    assert.equal(
      check(
        { anyOf: [{ type: "integer" }, { type: "number" }], maximum: 2 },
        3,
      ),
      false,
    );
    assert.equal(
      check({ allOf: [{ type: "integer" }, { minimum: 2 }] }, 1),
      false,
    );
    assert.equal(check({ const: { a: [1, true] } }, { a: [1, true] }), true);
    assert.equal(check({ const: true }, 1), false);
    assert.equal(
      check({ type: "integer" }, Number.MAX_SAFE_INTEGER + 1),
      false,
    );
    assert.equal(
      check(
        {
          type: "object",
          properties: { x: { type: "string" } },
          additionalProperties: false,
        },
        {},
      ),
      true,
    );
    assert.equal(
      check(
        {
          type: "object",
          properties: { x: { type: "string" } },
          additionalProperties: false,
        },
        { x: null },
      ),
      false,
    );
    assert.equal(
      check({ type: "object", additionalProperties: false }, { x: 1 }),
      false,
    );
    const recursive = { type: "array", items: { $ref: "#/$defs/tree" } };
    let nested: unknown = [];
    for (let index = 0; index < 150; index++) nested = [nested];
    assert.equal(
      validateContractSchema({ tree: recursive }, recursive, nested),
      false,
    );
  });
  test("distribution schema binds target to exact payload paths", () => {
    const schema = JSON.parse(
      readFileSync(
        "contracts/distribution/runtime-manifest.schema.json",
        "utf8",
      ),
    );
    const check = (value: unknown) =>
      validateContractSchema(schema.$defs, schema, value);
    for (const target of schema.properties.target.enum) {
      const kind = target.includes("windows") ? "windowsFiles" : "unixFiles";
      const files = Object.fromEntries(
        schema.$defs[kind].required.map((name: string) => [
          name,
          "a".repeat(64),
        ]),
      );
      const manifest = {
        schemaVersion: 1,
        protocolVersion: PROTOCOL_VERSION,
        version: "1.0.0",
        target,
        files,
      };
      assert.equal(check(manifest), true);
      manifest.target = target.includes("windows")
        ? "aarch64-unknown-linux-gnu"
        : "aarch64-pc-windows-msvc";
      assert.equal(check(manifest), false);
    }
  });

  test("accepts large assets while preserving base64 and reference validation", () => {
    const frame = JSON.parse(
      readFileSync("tests/fixtures/protocol/rust-render-frame.json", "utf8"),
    );
    const byteLength = 4 * 1024 * 1024 + 1;
    const data = Buffer.alloc(byteLength).toString("base64");
    frame.assets = ["7", "8"].map((digit) => ({
      reference: `fleximark-asset:${digit.repeat(64)}`,
      contentHash: digit.repeat(64),
      byteLength,
      data,
      mediaType: "image/avif",
    }));
    assert.equal(isRenderFrame(frame), true);
    assert.equal(
      customRequestResultValidators["fleximark/createPreview"]({
        previewSessionId: "preview",
        frame,
      }),
      true,
    );
    const original = frame.assets[0];
    for (const invalid of [
      { ...original, byteLength: byteLength - 1 },
      { ...original, byteLength: Number.MAX_SAFE_INTEGER + 1 },
      { ...original, reference: `fleximark-asset:${"9".repeat(64)}` },
      { ...original, contentHash: "invalid" },
      ...["A===", "AAAA=", "A=AA", "AA A", "%%%=", "AA=\n"].map(
        (malformed) => ({
          ...original,
          data: malformed,
          byteLength: 1,
        }),
      ),
    ]) {
      frame.assets[0] = invalid;
      assert.equal(isRenderFrame(frame), false);
    }
    frame.assets[0] = original;
    frame.assets[1] = original;
    assert.equal(isRenderFrame(frame), false);
  });

  test("accepts the frame serialized by the Rust renderer", () => {
    const frame = JSON.parse(
      readFileSync(
        path.join(
          process.cwd(),
          "tests",
          "fixtures",
          "protocol",
          "rust-render-frame.json",
        ),
        "utf8",
      ),
    ) as unknown;

    assert.equal(isRenderFrame(frame), true);
  });

  test("custom request validator registries match the shared fixture method set", () => {
    const fixture = JSON.parse(
      readFileSync(
        path.join(
          process.cwd(),
          "tests",
          "fixtures",
          "protocol",
          "contract.json",
        ),
        "utf8",
      ),
    ) as {
      methods: {
        method: string;
        kind: string;
        direction: "clientToServer" | "serverToClient" | "bidirectional";
        params: unknown;
        result: unknown;
      }[];
    };
    const requests = fixture.methods.filter(({ kind }) => kind === "request");
    const methods = requests.map(({ method }) => method).sort();

    assert.deepEqual(
      Object.keys(customRequestParamsValidators).sort(),
      methods,
    );
    assert.deepEqual(
      Object.keys(customRequestResultValidators).sort(),
      methods,
    );
    for (const request of requests) {
      const paramsValidator =
        customRequestParamsValidators[
          request.method as keyof typeof customRequestParamsValidators
        ];
      const resultValidator =
        customRequestResultValidators[
          request.method as keyof typeof customRequestResultValidators
        ];
      assert.equal(paramsValidator(request.params), true, request.method);
      assert.equal(resultValidator(request.result), true, request.method);
    }

    const notifications = fixture.methods.filter(
      ({ kind }) => kind === "notification",
    );
    const clientMethods = notifications
      .filter(({ direction }) => direction !== "serverToClient")
      .map(({ method }) => method)
      .sort();
    const serverMethods = notifications
      .filter(({ direction }) => direction !== "clientToServer")
      .map(({ method }) => method)
      .sort();
    const customKeys = (registry: object) =>
      Object.keys(registry)
        .filter((method) => method.startsWith("fleximark/"))
        .sort();
    assert.deepEqual(customKeys(clientNotificationValidators), clientMethods);
    assert.deepEqual(customKeys(serverNotificationValidators), serverMethods);

    for (const notification of notifications) {
      if (notification.direction !== "serverToClient")
        assert.equal(
          clientNotificationValidators[
            notification.method as keyof typeof clientNotificationValidators
          ](notification.params),
          true,
          notification.method,
        );
      if (notification.direction === "serverToClient")
        assert.equal(
          serverNotificationValidators[
            notification.method as keyof typeof serverNotificationValidators
          ](notification.params),
          true,
          notification.method,
        );
    }
  });

  test("attachDocument enforces JavaScript safe integer boundaries", () => {
    const validate = customRequestParamsValidators["fleximark/attachDocument"];
    const params = (expectedDocumentVersion: number) => ({
      daemonInstanceId: "daemon",
      uri: "file:///document.md",
      expectedDocumentVersion,
    });

    assert.equal(validate(params(-Number.MAX_SAFE_INTEGER)), true);
    assert.equal(validate(params(Number.MAX_SAFE_INTEGER)), true);
    assert.equal(validate(params(-Number.MAX_SAFE_INTEGER - 1)), false);
    assert.equal(validate(params(Number.MAX_SAFE_INTEGER + 1)), false);
  });

  test("JSON-RPC ids share the generated JavaScript-safe boundary", () => {
    const request = (id: unknown) => ({
      jsonrpc: "2.0",
      id,
      method: "unknown/request",
      params: {},
    });
    const response = (id: unknown) => ({
      jsonrpc: "2.0",
      id,
      result: null,
    });

    for (const id of [
      -Number.MAX_SAFE_INTEGER,
      Number.MAX_SAFE_INTEGER,
      "request-id",
    ]) {
      assert.equal(isJsonRpcMessageEnvelope(request(id)), true);
      assert.equal(isJsonRpcResponseEnvelope(response(id)), true);
    }
    for (const id of [
      -Number.MAX_SAFE_INTEGER - 1,
      Number.MAX_SAFE_INTEGER + 1,
      null,
    ])
      assert.equal(isJsonRpcMessageEnvelope(request(id)), false);
    assert.equal(isJsonRpcResponseEnvelope(response(null)), true);
    assert.equal(
      isJsonRpcResponseEnvelope(response(Number.MAX_SAFE_INTEGER + 1)),
      false,
    );
  });

  test("directional previewEvent validators accept only their wire direction", () => {
    const fixture = JSON.parse(
      readFileSync(
        path.join(
          process.cwd(),
          "tests",
          "fixtures",
          "protocol",
          "bidirectional-server.json",
        ),
        "utf8",
      ),
    ) as {
      notifications: { method: "fleximark/previewEvent"; params: unknown }[];
    };
    const source = {
      type: "selectSource",
      sourceRange: {
        byteStart: 0,
        byteEnd: 0,
        start: { line: 0, character: 0, encoding: "utf8" },
        end: { line: 0, character: 0, encoding: "utf8" },
      },
    };
    const selectionWithNullActive = {
      type: "selection",
      previewSessionId: "preview",
      renderRevision: 1,
      nodeIds: ["document-root"],
      activePosition: null,
    };
    const viewport = {
      type: "viewport",
      previewSessionId: "preview",
      renderRevision: 1,
      nodeId: "document-root",
    };
    const client = {
      type: "selectNode",
      previewSessionId: "preview",
      renderRevision: 1,
      nodeId: "document-root",
    };
    const serverEvents = [source, selectionWithNullActive, viewport];

    for (const event of serverEvents) {
      const params = {
        daemonInstanceId: "daemon",
        previewSessionId: "preview",
        renderRevision: 1,
        event,
      };
      assert.equal(
        serverNotificationValidators["fleximark/previewEvent"](params),
        true,
      );
      assert.equal(
        clientNotificationValidators["fleximark/previewEvent"](params),
        false,
      );
    }
    const clientParams = {
      daemonInstanceId: "daemon",
      previewSessionId: "preview",
      renderRevision: 1,
      event: client,
    };
    assert.equal(
      clientNotificationValidators["fleximark/previewEvent"](clientParams),
      true,
    );
    assert.equal(
      serverNotificationValidators["fleximark/previewEvent"](clientParams),
      false,
    );
    for (const { method, params } of fixture.notifications) {
      assert.equal(serverNotificationValidators[method](params), true, method);
      assert.equal(clientNotificationValidators[method](params), false, method);
    }
  });

  test("accepts every legal optional-present DTO from the shared fixture", () => {
    const fixture = JSON.parse(
      readFileSync(
        path.join(
          process.cwd(),
          "tests",
          "fixtures",
          "protocol",
          "optional-present.json",
        ),
        "utf8",
      ),
    ) as { cases: { name: string; definition: string; value: unknown }[] };
    const validators: Record<string, (value: unknown) => boolean> = {
      initializeParams: (value) => hasContractShape("initializeParams", value),
      initializeResult: (value) => hasContractShape("initializeResult", value),
      createPreviewResult:
        customRequestResultValidators["fleximark/createPreview"],
      executeCommandParams:
        customRequestParamsValidators["fleximark/executeCommand"],
      commandResult: customRequestResultValidators["fleximark/executeCommand"],
    };

    assert.deepEqual(
      [...new Set(fixture.cases.map(({ definition }) => definition))].sort(),
      Object.keys(validators).sort(),
    );
    for (const item of fixture.cases)
      assert.equal(validators[item.definition](item.value), true, item.name);
  });

  test("rejects invalid present note category paths", () => {
    const validate = customRequestParamsValidators["fleximark/executeCommand"];
    const base = { daemonInstanceId: "daemon", command: "createNote" };
    assert.equal(validate(base), true);
    for (const noteCategoryPath of [
      null,
      [],
      Array.from({ length: 33 }, () => "x"),
      ["Work", ""],
    ]) {
      assert.equal(validate({ ...base, noteCategoryPath }), false);
    }
  });

  test("rejects invalid present note file names", () => {
    const validate = customRequestParamsValidators["fleximark/executeCommand"];
    const base = { daemonInstanceId: "daemon", command: "createNote" };
    for (const noteFileName of [null, "", "x".repeat(256)])
      assert.equal(validate({ ...base, noteFileName }), false);
    assert.equal(validate({ ...base, noteFileName: "session" }), true);
  });
}
