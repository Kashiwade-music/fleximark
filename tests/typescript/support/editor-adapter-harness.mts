import { FlexiMarkAdapter } from "fleximark/src/adapter";
import { createAdapterServices } from "fleximark/src/adapter-services";
import {
  type FlexiMarkTestApi,
  createExtensionTestApi,
} from "fleximark/src/extension-test-api";
import type * as vscode from "vscode";

/** Keeps editor-only inspection in the test layer and explicit extension API. */
export class TestAdapter extends FlexiMarkAdapter {
  readonly #api: FlexiMarkTestApi;
  constructor(context: vscode.ExtensionContext) {
    const services = createAdapterServices(context);
    super(context, services);
    const api = createExtensionTestApi(context, services);
    if (!api) throw new Error("Editor harness requires ExtensionMode.Test");
    this.#api = api;
  }
  crashDaemonForTest(): void {
    this.#api.crashDaemon();
  }
  recoveryStateForTest() {
    return this.#api.recoveryState();
  }
  openExternalPreviewForTest(): Promise<string> {
    return this.#api.openExternalPreview();
  }
}
