import * as assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import * as path from "node:path";

import {
  clientNotificationValidators,
  customRequestParamsValidators,
  customRequestResultValidators,
  serverNotificationValidators,
} from "../web/preview-client/protocol.mjs";

export const suiteName = "TypeScript protocol contract";

export function suite(): void {
  test("custom request validator registries match the shared fixture method set", () => {
    const fixture = JSON.parse(
      readFileSync(
        path.join(
          process.cwd(),
          "test",
          "fixtures",
          "protocol-v1-contract.json",
        ),
        "utf8",
      ),
    ) as {
      methods: {
        method: string;
        kind: string;
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
    const notificationMethods = notifications
      .map(({ method }) => method)
      .sort();
    const registryMethods = [
      ...Object.keys(clientNotificationValidators),
      ...Object.keys(serverNotificationValidators).filter(
        (method) => method !== "textDocument/publishDiagnostics",
      ),
    ];
    assert.deepEqual([...new Set(registryMethods)].sort(), notificationMethods);

    for (const notification of notifications) {
      const validator =
        notification.method === "fleximark/requestFullText"
          ? serverNotificationValidators["fleximark/requestFullText"]
          : clientNotificationValidators[
              notification.method as keyof typeof clientNotificationValidators
            ];
      assert.equal(validator(notification.params), true, notification.method);
    }
  });

  test("directional previewEvent validators accept only their wire direction", () => {
    const snapshot = {
      type: "full",
      previewSessionId: "preview",
      documentVersion: 1,
      resultRenderRevision: 1,
      rendererFingerprint: "sha256:renderer",
      nodeIds: ["document-root"],
      navigation: [],
      style: null,
      assets: [],
      html: '<main data-fleximark-node-id="document-root"></main>',
    };
    const patch = {
      type: "patch",
      previewSessionId: "preview",
      documentVersion: 2,
      baseRenderRevision: 1,
      resultRenderRevision: 2,
      baseRendererFingerprint: "sha256:renderer",
      resultRendererFingerprint: "sha256:renderer",
      navigation: [],
      style: null,
      operations: [],
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
    const selectionWithoutActive = {
      type: "selection",
      previewSessionId: "preview",
      renderRevision: 1,
      nodeIds: ["document-root"],
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
    const serverEvents = [
      snapshot,
      patch,
      source,
      selectionWithoutActive,
      viewport,
    ];

    for (const event of serverEvents) {
      const revision = event.type === "patch" ? 2 : 1;
      const params = {
        daemonInstanceId: "daemon",
        previewSessionId: "preview",
        renderRevision: revision,
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
  });

  test("accepts every legal optional-present DTO from the shared fixture", () => {
    const fixture = JSON.parse(
      readFileSync(
        path.join(
          process.cwd(),
          "test",
          "fixtures",
          "protocol-v1-optional-present.json",
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
