from __future__ import annotations

import shutil
import tempfile
from pathlib import Path

from _tools import ROOT, run, script_entrypoint


SOURCE_EXTENSIONS = {".mts", ".cts", ".ts", ".js"}


def export_localization() -> None:
    source_root = ROOT / "adapters" / "vscode" / "src"
    output_root = ROOT / "l10n"
    with tempfile.TemporaryDirectory(prefix=".fleximark-l10n-", dir=ROOT) as temp:
        temporary_source = Path(temp) / "src"
        for source in source_root.rglob("*"):
            if not source.is_file() or source.suffix not in SOURCE_EXTENSIONS:
                continue
            relative = source.relative_to(source_root)
            target_name = (
                relative.with_suffix(".ts")
                if relative.suffix in {".mts", ".cts"}
                else relative
            )
            target = temporary_source / target_name
            target.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(source, target)

        run(
            "node",
            "node_modules/@vscode/l10n-dev/dist/cli.js",
            "export",
            "--outDir",
            str(output_root),
            str(temporary_source),
        )

    bundle = output_root / "bundle.l10n.json"
    contents = bundle.read_text(encoding="utf-8")
    if not contents.endswith("\n"):
        contents += "\n"
    bundle.write_bytes(contents.encode("utf-8"))


if __name__ == "__main__":
    script_entrypoint(export_localization)
