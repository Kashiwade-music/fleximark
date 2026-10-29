import { WORKSPACE_COMMAND_KIND } from "@fleximark/contracts/protocol";
import type {
  CommandResult,
  WorkspaceStateResult,
  WorkspaceUiCommandRequest,
} from "@fleximark/contracts/protocol.generated";

import {
  browserCommands,
  categoryPaths,
  isCommandResult,
  isWorkspaceState,
  migrationSummary,
  readResponse,
  runWorkspaceCommand,
  validateFileName,
} from "./model.mjs";

const root = document.querySelector<HTMLElement>("#workspace");
if (!root) throw new Error("Workspace root is missing");
const base = location.pathname.replace(/\/$/, "");
root.innerHTML = `<h1>FlexiMark workspace</h1><form>
<label>Workspace<select name="workspace"><option value="">Select a workspace</option></select></label>
<label>Document<select name="document"><option value="">Select a document</option></select></label>
<label>Operation<select name="command"></select></label>
<fieldset id="note-fields"><legend>New note</legend><label>Category<select name="category"></select></label><label>Template<select name="template"></select></label><label>File name<input name="fileName" maxlength="255" placeholder="my-note.md"></label></fieldset>
<fieldset id="export-fields" hidden><legend>Export</legend><label>Destination file URI (optional)<input name="destination" placeholder="file:///path/document.html"></label></fieldset>
<button type="submit">Run operation</button><button type="button" id="refresh">Refresh</button></form>
<p id="status" role="status" aria-live="polite"></p><div id="result"></div>`;
function required<T extends Element>(selector: string): T {
  const element = document.querySelector<T>(selector);
  if (!element) throw new Error("Workspace element is missing: " + selector);
  return element;
}
const form = required<HTMLFormElement>("#workspace form");
const control = (name: string) =>
  form.elements.namedItem(name) as HTMLSelectElement | HTMLInputElement;
const status = required<HTMLElement>("#status");
const result = required<HTMLElement>("#result");
const commandSelect = control("command") as HTMLSelectElement;
commandSelect.replaceChildren(
  ...browserCommands
    .filter(({ kind }) => kind !== WORKSPACE_COMMAND_KIND.applyMigration)
    .map(({ name, title }) => new Option(title, name)),
);
let state: WorkspaceStateResult = {
  context: {},
  workspaces: [],
  documents: [],
};
let loading = false,
  request = 0,
  initial = true;
let ready = false;
function options(
  name: string,
  items: { value: string; label: string }[],
  placeholder: string,
): void {
  const select = control(name) as HTMLSelectElement,
    selected = select.value;
  select.replaceChildren(
    new Option(placeholder, ""),
    ...items.map((i) => new Option(i.label, i.value)),
  );
  if (items.some((i) => i.value === selected)) select.value = selected;
}
function updateFields(): void {
  required<HTMLElement>("#note-fields").hidden = !browserCommands.some(
    ({ name, kind }) =>
      name === control("command").value &&
      kind === WORKSPACE_COMMAND_KIND.createNote,
  );
  required<HTMLElement>("#export-fields").hidden = !browserCommands.some(
    ({ name, kind }) =>
      name === control("command").value &&
      (kind === WORKSPACE_COMMAND_KIND.exportHtml ||
        kind === WORKSPACE_COMMAND_KIND.acknowledgeExport),
  );
}
function populate(): void {
  options(
    "workspace",
    state.workspaces.map((w) => ({ value: w.uri, label: w.name })),
    "Select a workspace",
  );
  const workspace = control("workspace").value;
  options(
    "document",
    state.documents
      .filter((d) => !workspace || d.workspaceUri === workspace)
      .map((d) => ({ value: d.uri, label: d.uri })),
    "Select a document",
  );
  options(
    "category",
    categoryPaths(state.noteOptions?.categories ?? []).map((p) => ({
      value: JSON.stringify(p),
      label: p.join(" / "),
    })),
    "Workspace root",
  );
  options(
    "template",
    (state.noteOptions?.templates ?? []).map((t) => ({ value: t, label: t })),
    "Default template",
  );
}
async function refresh(): Promise<void> {
  ready = false;
  const generation = ++request,
    query = new URLSearchParams();
  if (control("workspace").value)
    query.set("workspaceUri", control("workspace").value);
  if (control("document").value)
    query.set("documentUri", control("document").value);
  status.textContent = "Loading workspace options…";
  try {
    const next = await readResponse(
      await fetch(base + "/state?" + query, { credentials: "same-origin" }),
      isWorkspaceState,
    );
    if (generation !== request) return;
    state = next;
    populate();
    if (initial) {
      const contextDocument = state.documents.find(
        (d) => d.uri === state.context?.documentUri,
      );
      if (contextDocument?.workspaceUri)
        control("workspace").value = contextDocument.workspaceUri;
      if (state.context?.workspaceUri)
        control("workspace").value = state.context.workspaceUri;
      populate();
      if (state.context?.documentUri)
        control("document").value = state.context.documentUri;
      if (
        state.command &&
        Array.from((control("command") as HTMLSelectElement).options).some(
          (o) => o.value === state.command,
        )
      )
        control("command").value = state.command;
      initial = false;
      updateFields();
    }
    ready = true;
    status.textContent = state.workspaces.length
      ? "Choose context to continue."
      : "No workspaces are open in the editor.";
  } catch (error) {
    if (generation === request)
      status.textContent =
        error instanceof Error ? error.message : String(error);
  }
}
control("command").addEventListener("change", updateFields);
control("workspace").addEventListener("change", () => {
  populate();
  void refresh();
});
control("document").addEventListener("change", () => {
  void refresh();
});
required<HTMLElement>("#refresh").addEventListener("click", () => {
  if (!loading) void refresh();
});
form.addEventListener("submit", (event) => {
  event.preventDefault();
  if (loading) return;
  if (!ready) {
    status.textContent =
      "Refresh workspace options before running an operation.";
    return;
  }
  const descriptor = browserCommands.find(
    ({ name }) => name === control("command").value,
  );
  if (!descriptor) return;
  const workspaceUri = control("workspace").value;
  const doc = state.documents.find((d) => d.uri === control("document").value);
  if (descriptor.requiresWorkspace && !workspaceUri) {
    status.textContent = "Select a workspace.";
    return;
  }
  if (descriptor.requiresDocument && !doc) {
    status.textContent = "Select a document.";
    return;
  }
  const context: WorkspaceUiCommandRequest["context"] = {
    ...(workspaceUri ? { workspaceUri } : {}),
    ...(doc ? { documentUri: doc.uri, documentVersion: doc.version } : {}),
  };
  let operation: WorkspaceUiCommandRequest["operation"];
  switch (descriptor.kind) {
    case WORKSPACE_COMMAND_KIND.createNote: {
      const name = control("fileName").value.trim();
      if (!validateFileName(name)) {
        status.textContent = "Enter a valid file name without path separators.";
        return;
      }
      operation = {
        command: descriptor.name,
        name,
        ...(control("category").value
          ? { categoryPath: JSON.parse(control("category").value) as string[] }
          : {}),
        ...(control("template").value
          ? { template: control("template").value }
          : {}),
      };
      break;
    }
    case WORKSPACE_COMMAND_KIND.exportHtml:
    case WORKSPACE_COMMAND_KIND.acknowledgeExport: {
      const destinationUri = control("destination").value.trim();
      if (destinationUri && !destinationUri.startsWith("file:///")) {
        status.textContent = "Destination must be a file URI.";
        return;
      }
      operation = {
        command: descriptor.name,
        ...(destinationUri ? { destinationUri } : {}),
      };
      break;
    }
    case WORKSPACE_COMMAND_KIND.planMigration:
      operation = { command: descriptor.name, recover: false };
      break;
    case WORKSPACE_COMMAND_KIND.applyMigration:
      return;
    default:
      operation = { command: descriptor.name };
  }
  const input: WorkspaceUiCommandRequest = { context, operation };
  const generation = request;
  loading = true;
  form.setAttribute("aria-busy", "true");
  for (const button of form.querySelectorAll("button")) button.disabled = true;
  status.textContent = "Running operation…";
  result.replaceChildren();
  void (async () => {
    try {
      const send = async (input: WorkspaceUiCommandRequest) =>
        readResponse(
          await fetch(base + "/command", {
            method: "POST",
            credentials: "same-origin",
            headers: { "Content-Type": "application/json" },
            body: JSON.stringify(input),
          }),
          isCommandResult,
        );
      const response = await runWorkspaceCommand(
        input,
        send,
        (plan) => {
          const summary = migrationSummary(plan);
          const pre = document.createElement("pre");
          pre.textContent = summary;
          result.replaceChildren(pre);
          return window.confirm(summary + "\nApply this migration?");
        },
        () => generation === request,
      );
      if (generation !== request) return;
      if (!response) {
        status.textContent = "Operation cancelled.";
        return;
      }
      status.textContent = response.message?.text ?? "Operation completed.";
      const uri = resultUri(response);
      if (uri) {
        const url = new URL(uri);
        if (["http:", "https:", "file:"].includes(url.protocol)) {
          const link = document.createElement("a");
          link.textContent = "Open result";
          link.href = url.href;
          link.rel = "noopener";
          if (response.kind === "htmlExported")
            link.addEventListener("click", () => {
              const acknowledgement = browserCommands.find(
                ({ kind }) => kind === WORKSPACE_COMMAND_KIND.acknowledgeExport,
              );
              if (
                !acknowledgement ||
                acknowledgement.kind !==
                  WORKSPACE_COMMAND_KIND.acknowledgeExport ||
                generation !== request
              )
                return;
              void send({
                context,
                operation: {
                  command: acknowledgement.name,
                  destinationUri: response.outputUri,
                },
              }).catch((error: unknown) => {
                status.textContent =
                  error instanceof Error ? error.message : String(error);
              });
            });
          result.append(link);
        }
      }
      const pre = document.createElement("pre");
      pre.textContent =
        response.kind === "migrationPlanned"
          ? migrationSummary(response.plan)
          : JSON.stringify(response, null, 2);
      result.append(pre);
    } catch (error) {
      status.textContent =
        error instanceof Error ? error.message : String(error);
    } finally {
      loading = false;
      form.removeAttribute("aria-busy");
      for (const button of form.querySelectorAll("button"))
        button.disabled = false;
    }
  })();
});
updateFields();
void refresh();

function resultUri(response: CommandResult): string | undefined {
  switch (response.kind) {
    case "workspaceInitialized":
      return response.configUri;
    case "themeEdited":
    case "noteCreated":
    case "admonitionsCollected":
      return response.uri;
    case "htmlExported":
      return response.outputUri;
    case "workspaceOpened":
      return response.url;
    case "previewOpened":
      return response.preview.url;
    default:
      return;
  }
}
