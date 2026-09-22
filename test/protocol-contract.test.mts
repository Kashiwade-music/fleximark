import * as assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import * as path from "node:path";

import {
  clientNotificationValidators,
  customRequestParamsValidators,
  customRequestResultValidators,
  isJsonRpcMessageEnvelope,
  isJsonRpcResponseEnvelope,
  isRenderFrame,
  serverNotificationValidators,
} from "../web/preview-client/protocol.mjs";

export const suiteName = "TypeScript protocol contract";

export function suite(): void {
  test("accepts the frame serialized by the Rust renderer", () => {
    const frame = JSON.parse(
      readFileSync(
        path.join(
          process.cwd(),
          "test",
          "fixtures",
          "rust-render-frame-v2.json",
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
          "test",
          "fixtures",
          "protocol-v2-contract.json",
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

  test("openDocument enforces JavaScript safe integer boundaries", () => {
    const validate = customRequestParamsValidators["fleximark/openDocument"];
    const params = (documentVersion: number) => ({
      daemonInstanceId: "daemon",
      uri: "file:///document.md",
      documentVersion,
      text: "text",
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
          "test",
          "fixtures",
          "protocol-v2-bidirectional-server.json",
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
          "test",
          "fixtures",
          "protocol-v2-optional-present.json",
        ),
        "utf8",
      ),
    ) as { cases: { name: string; definition: string; value: unknown }[] };
    const validators: Record<string, (value: unknown) => boolean> = {
      initializeParams: customRequestParamsValidators["fleximark/initialize"],
      initializeResult: customRequestResultValidators["fleximark/initialize"],
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
}
