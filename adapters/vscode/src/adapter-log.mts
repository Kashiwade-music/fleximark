import * as vscode from "vscode";

import { errorMessage, redactSensitiveText } from "./error-policy.mjs";

/** Filters daemon diagnostics before writing to the extension output channel. */
export class AdapterLog {
  readonly output = vscode.window.createOutputChannel("FlexiMark");

  write(message: string, level: "error" | "info" | "debug" = "info"): void {
    const configured = vscode.workspace
      .getConfiguration("fleximark")
      .get<string>("logLevel", "info");
    if (
      configured === "off" ||
      (configured === "error" && level !== "error") ||
      (configured !== "debug" && level === "debug")
    )
      return;
    this.output.appendLine(redactSensitiveText(message));
  }

  stderr(message: string): void {
    this.write(
      message,
      /\b(error|panic)\b/i.test(message)
        ? "error"
        : /\b(debug|trace)\b/i.test(message)
          ? "debug"
          : "info",
    );
  }

  report(error: unknown): void {
    const message = redactSensitiveText(errorMessage(error));
    this.write(message, "error");
    void vscode.window.showErrorMessage(message);
  }

  dispose(): void {
    this.output.dispose();
  }
}
