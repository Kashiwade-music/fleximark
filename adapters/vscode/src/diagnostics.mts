import * as vscode from "vscode";

import {
  type PublishDiagnosticsParams,
  serverNotificationValidators,
} from "./protocol.mjs";

export function isPublishDiagnosticsParams(
  value: unknown,
): value is PublishDiagnosticsParams {
  return serverNotificationValidators["textDocument/publishDiagnostics"](value);
}

export function applyPublishedDiagnostics(
  collection: vscode.DiagnosticCollection,
  params: PublishDiagnosticsParams,
): void {
  collection.set(
    vscode.Uri.parse(params.uri),
    params.diagnostics.map((item) => {
      const diagnostic = new vscode.Diagnostic(
        new vscode.Range(
          item.range.start.line,
          item.range.start.character,
          item.range.end.line,
          item.range.end.character,
        ),
        item.message,
        item.severity === 1
          ? vscode.DiagnosticSeverity.Error
          : vscode.DiagnosticSeverity.Warning,
      );
      diagnostic.code = item.code;
      diagnostic.source = item.source;
      (diagnostic as vscode.Diagnostic & { data?: unknown }).data = item.data;
      return diagnostic;
    }),
  );
}
