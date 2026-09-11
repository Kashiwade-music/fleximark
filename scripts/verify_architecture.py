from __future__ import annotations

import json
import re
from pathlib import Path
from typing import Any

from _tools import ROOT


def read_json(path: Path) -> dict[str, Any]:
    return json.loads(path.read_text(encoding="utf-8"))


def require(condition: bool, message: str) -> None:
    if not condition:
        raise RuntimeError(message)


def verify_architecture() -> None:
    package_json = read_json(ROOT / "package.json")
    inventory = read_json(ROOT / "capabilities" / "feature-inventory.json")
    disposition_catalog = read_json(
        ROOT / "capabilities" / "clean-break-catalog.json"
    )
    adapter_source = "\n".join(
        (ROOT / "adapters" / "vscode" / "src" / file).read_text(
            encoding="utf-8"
        )
        for file in ("adapter.mts", "extension.mts")
    )
    rust_protocol = (ROOT / "crates" / "fleximark-protocol" / "src" / "lib.rs").read_text(
        encoding="utf-8"
    )

    require(package_json.get("main") == "./dist/extension.cjs", "unexpected extension entry point")
    require(
        package_json.get("activationEvents")
        == ["onLanguage:markdown", "workspaceContains:.fleximark/config.toml"],
        "unexpected activation events",
    )
    properties = package_json["contributes"]["configuration"]["properties"]
    for key in (
        "fleximark.browserPreviewPort",
        "fleximark.shouldSyncScroll",
        "fleximark.noteCategories",
        "fleximark.noteFileNamePrefix",
        "fleximark.noteFileNameSuffix",
        "fleximark.noteTemplates",
    ):
        require(key not in properties, f"{key} is service-owned")

    for dependency in package_json.get("dependencies", {}):
        require(
            re.search(r"^(remark|rehype|shiki|express$|ws$)", dependency) is None,
            f"{dependency} belongs outside the adapter",
        )

    require(inventory.get("schemaVersion") == 1, "unsupported inventory schema")
    capabilities = inventory["capabilities"]
    identifiers = [capability["id"] for capability in capabilities]
    require(len(set(identifiers)) == len(identifiers), "capability IDs must be unique")
    for capability in capabilities:
        require(bool(capability.get("owner")), f"{capability['id']} needs an owner")

    for clean_break_id, disposition in disposition_catalog["cleanBreaks"].items():
        require(
            all(
                disposition.get(field)
                for field in ("rationale", "replacement", "unsupportedBehavior")
            ),
            f"{clean_break_id} is incomplete",
        )
        require(
            disposition["releaseNoteId"] in disposition_catalog["releaseNotes"],
            f"{clean_break_id} references an unknown release note",
        )

    for method in set(re.findall(r"fleximark/[A-Za-z]+", adapter_source)):
        require(f'"{method}"' in rust_protocol, f"{method} is missing from fleximark-protocol")

    for removed_runtime in ("src", "media"):
        directory = ROOT / removed_runtime
        require(
            not directory.exists() or not any(path.is_file() for path in directory.rglob("*")),
            f"{removed_runtime}: legacy TypeScript runtime must not be shipped",
        )
    require(
        not (ROOT / "parserPlugin.js").exists(),
        "parserPlugin.js: legacy TypeScript runtime must not be shipped",
    )

    for schema in (
        "config.schema.json",
        "protocol.schema.json",
        "plugin-manifest.schema.json",
        "feature-inventory.schema.json",
    ):
        document = read_json(ROOT / "schemas" / schema)
        require(
            document.get("$schema") == "https://json-schema.org/draft/2020-12/schema",
            f"{schema} uses an unexpected JSON Schema draft",
        )

    protocol_schema = read_json(ROOT / "schemas" / "protocol.schema.json")
    for dto in (
        "initializeParams",
        "initializeResult",
        "attachDocumentParams",
        "attachDocumentResult",
        "checkpointDocumentParams",
        "checkpointDocumentResult",
        "renderParams",
        "renderPublication",
        "renderStyle",
        "previewNavigationEvent",
        "sourceNavigationEvent",
        "renderNavigationEvent",
        "createPreviewParams",
        "createPreviewResult",
        "previewSessionParams",
        "selectionParams",
        "viewportParams",
        "previewEventParams",
        "executeCommandParams",
        "getNoteOptionsParams",
        "getNoteOptionsResult",
        "commandResult",
        "openDocumentParams",
        "changeDocumentParams",
        "requestFullTextParams",
        "closeDocumentParams",
        "methodResults",
    ):
        require(dto in protocol_schema["$defs"], f"protocol DTO {dto} is missing")

    serialized_schema = json.dumps(protocol_schema)
    for method in re.findall(
        r'pub const [A-Z_]+: &str = "(fleximark/[A-Za-z]+)";', rust_protocol
    ):
        require(method in serialized_schema, f"{method} is missing from protocol.schema.json")
    require("params?: unknown" not in adapter_source, "adapter uses an untyped params object")

    print(f"Verified {len(capabilities)} current capabilities.")


if __name__ == "__main__":
    verify_architecture()
