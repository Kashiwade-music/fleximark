"""Assemble versioned native runtimes and editor products from independent outputs."""
from __future__ import annotations

import argparse
import hashlib
import json
import platform
import re
import shutil
import sys
import zipfile
from pathlib import Path

from _tools import ROOT, run, script_entrypoint
from distribution_contract import SPEC, TARGETS, metadata, normalize_arch, normalize_platform, read_manifest, target_spec, verify_runtime


def host_target() -> str:
    return TARGETS[f"{normalize_platform(sys.platform)}-{normalize_arch(platform.machine())}"]


def staging(target: str, product: str) -> Path:
    if target not in TARGETS.values():
        raise RuntimeError(f"unsupported distribution target: {target}")
    return ROOT / ".artifacts" / metadata()["version"] / target / product


def digest(path: Path) -> str:
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def reset_output(path: Path) -> None:
    resolved = path.resolve()
    if not resolved.is_relative_to((ROOT / ".artifacts").resolve()):
        raise RuntimeError(f"refusing cleanup outside artifact staging: {path}")
    shutil.rmtree(resolved, ignore_errors=True)
    resolved.mkdir(parents=True)


def generate_versions() -> None:
    version = metadata()["version"]
    path = ROOT / "adapters" / "vscode" / "package.json"
    source = path.read_text(encoding="utf-8")
    updated = re.sub(r'"version": "[^"]+"', f'"version": "{version}"', source, count=1)
    if updated != source:
        path.write_text(updated, encoding="utf-8", newline="\n")
    cargo_changed = False
    for owner in ("server", "cli"):
        cargo = ROOT / "apps" / owner / "Cargo.toml"
        source = cargo.read_text(encoding="utf-8")
        updated = re.sub(r'^version = "[^"]+"', f'version = "{version}"', source, count=1, flags=re.M)
        if updated != source:
            cargo.write_text(updated, encoding="utf-8", newline="\n")
            cargo_changed = True
    if cargo_changed:
        run("cargo", "update", "--workspace", "--offline")
    path = ROOT / "adapters" / "zed" / "extension.toml"
    if path.is_file():
        source = path.read_text(encoding="utf-8")
        updated = re.sub(r'^version = "[^"]+"', f'version = "{version}"', source, count=1, flags=re.M)
        if updated != source:
            path.write_text(updated, encoding="utf-8", newline="\n")


def build_native(target: str | None = None) -> None:
    args = ["build", "--release", "--locked", "--bin", "fleximarkd", "--bin", "fleximark"]
    if target is not None:
        args += ["--target", target]
    run("cargo", *args)


def assemble_runtime(target: str, *, native_dir: Path | None = None, preview_dir: Path | None = None) -> Path:
    output = staging(target, "runtime")
    reset_output(output)
    native = native_dir or ROOT / "target" / target / "release"
    preview = preview_dir or ROOT / ".build" / "preview"
    descriptor = target_spec(target)
    sources = {"native": native, "preview": preview, "workspace": ROOT / ".build" / "workspace-ui"}
    prefixes = {"native": "bin/", "preview": "resources/preview/", "workspace": "resources/workspace/"}
    for file in SPEC["files"]:
        relative = file["path"].replace("{exe}", descriptor["executableSuffix"])
        source = sources[file["source"]] / relative.removeprefix(prefixes[file["source"]])
        destination = output / relative
        destination.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(source, destination)
        if file["source"] == "native" and not descriptor["executableSuffix"]:
            destination.chmod(0o755)
    files = {p.relative_to(output).as_posix(): digest(p) for p in sorted(output.rglob("*")) if p.is_file()}
    release = metadata()
    manifest = {"schemaVersion": 1, "version": release["version"], "target": target, "protocolVersion": release["protocol_version"], "files": files}
    (output / "manifest.json").write_text(json.dumps(manifest, indent=2) + "\n", encoding="utf-8")
    archive = output.parent / f'fleximark-{release["version"]}-{target}.zip'
    with zipfile.ZipFile(archive, "w", compression=zipfile.ZIP_DEFLATED) as stream:
        for path in sorted(output.rglob("*")):
            if path.is_file():
                stream.write(path, path.relative_to(output).as_posix())
    archive.with_suffix(".zip.sha256").write_text(f"{digest(archive)}  {archive.name}\n", encoding="utf-8")
    return archive


def assemble_vscode(target: str, archive: Path) -> Path:
    """Consume the exact native archive distributed to other editors."""
    output = staging(target, "vscode")
    reset_output(output)
    source = ROOT / "adapters" / "vscode"
    for name in ("package.json", "package.nls.json", "package.nls.ja.json", "README.md", "l10n", "languages", "assets", ".vscodeignore"):
        path = source / name
        if path.is_dir():
            shutil.copytree(path, output / name)
        elif path.is_file():
            shutil.copy2(path, output / name)
    shutil.copy2(ROOT / "LICENSE", output / "LICENSE")
    package_path = output / "package.json"
    package = json.loads(package_path.read_text(encoding="utf-8"))
    package.pop("scripts", None)
    package_path.write_text(json.dumps(package, indent=2) + "\n", encoding="utf-8")
    shutil.copytree(ROOT / ".build" / "vscode" / "dist", output / "dist")
    (output / "dist" / "webview").mkdir(exist_ok=True)
    shutil.copy2(ROOT / ".build" / "preview" / "vscode-host.js", output / "dist" / "webview" / "vscode-host.js")
    runtime = output / "runtime"
    runtime.mkdir()
    with zipfile.ZipFile(archive) as stream:
        from release_artifact import runtime_manifest_artifacts
        if stream.getinfo("manifest.json").file_size > SPEC["limits"]["manifestBytes"]:
            raise RuntimeError("runtime manifest exceeds size limit")
        payload = stream.read("manifest.json")
        expected = {item["path"].removeprefix("runtime/") for item in runtime_manifest_artifacts(payload, "runtime archive manifest")} | {"manifest.json"}
        if set(stream.namelist()) != expected or len(stream.namelist()) != len(expected):
            raise RuntimeError("runtime archive does not match canonical file set")
        if sum(item.file_size for item in stream.infolist()) > 512 * SPEC["limits"]["manifestBytes"]:
            raise RuntimeError("runtime archive exceeds total size limit")
        for item in stream.infolist():
            if item.filename.startswith("/") or ".." in Path(item.filename).parts or "\\" in item.filename:
                raise RuntimeError("unsafe runtime archive member")
            if item.file_size > 256 * 1024 * 1024 or item.file_size > max(1, item.compress_size) * 200:
                raise RuntimeError("runtime archive member exceeds resource limits")
        stream.extractall(runtime)
    if "windows" not in target:
        for name in ("fleximarkd", "fleximark"):
            (runtime / "bin" / name).chmod(0o755)
    verify_runtime(runtime, target)
    return output


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("command", choices=("versions", "native", "runtime", "vscode"))
    parser.add_argument("--target", default=host_target())
    parser.add_argument("--native-dir", type=Path)
    parser.add_argument("--archive", type=Path)
    args = parser.parse_args()
    if args.command == "versions":
        generate_versions()
    elif args.command == "native":
        generate_versions()
        build_native(args.target)
    elif args.command == "runtime":
        print(assemble_runtime(args.target, native_dir=args.native_dir))
    else:
        if args.archive is None:
            parser.error("vscode requires --archive")
        print(assemble_vscode(args.target, args.archive))


if __name__ == "__main__":
    script_entrypoint(main)
