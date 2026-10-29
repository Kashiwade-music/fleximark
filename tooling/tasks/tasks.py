from __future__ import annotations

import argparse
import os
import shutil
import subprocess
import threading
import time
from collections.abc import Callable, Sequence
from dataclasses import dataclass
from pathlib import Path
import json

import build as javascript_build
from _tools import ROOT, executable, run, script_entrypoint, yarn
from check_performance_budgets import check_performance_budgets
from l10n_export import export_localization
from protocol_codegen import check_protocol_contract, generate_protocol_contract
from release_artifact import validate_prebuilt_inputs
from smoke_vsix import smoke_vsix
from distribution import assemble_runtime, assemble_vscode, build_native, generate_versions, host_target, metadata


def clean(_: Sequence[str]) -> None:
    javascript_build.clean()


def build_daemon() -> None:
    build_native(host_target())


def build_runtime() -> Path:
    Plan(runtime_build_steps(), "Native runtime and browser resources assembled.").execute()
    return runtime_archive()


def build_vscode_product(archive: Path) -> None:
    javascript_build.build_extension(production=True)
    assemble_vscode(host_target(), archive)


def build_zed_product() -> None:
    from distribution import staging, reset_output
    run("cargo", "build", "--manifest-path", "adapters/zed/Cargo.toml", "--release", "--target", "wasm32-wasip2", "--target-dir", str(ROOT / "target" / "zed"), "--locked")
    output = staging(host_target(), "zed")
    reset_output(output)
    shutil.copy2(ROOT / "adapters" / "zed" / "extension.toml", output / "extension.toml")
    shutil.copy2(ROOT / "target" / "zed" / "wasm32-wasip2" / "release" / "fleximark_zed.wasm", output / "extension.wasm")
    languages = ROOT / "adapters" / "zed" / "languages"
    if languages.is_dir():
        shutil.copytree(languages, output / "languages")
    from distribution import digest
    import zipfile
    archive = output.parent / f'fleximark-zed-{metadata()["version"]}.zip'
    with zipfile.ZipFile(archive, "w", compression=zipfile.ZIP_DEFLATED) as stream:
        for path in sorted(output.rglob("*")):
            if path.is_file():
                stream.write(path, path.relative_to(output).as_posix())
    archive.with_suffix(".zip.sha256").write_text(f"{digest(archive)}  {archive.name}\n", encoding="utf-8")


def build_core(_: Sequence[str]) -> None:
    run("cargo", "build", "--locked", "-p", "fleximark-model", "-p", "fleximark-parser", "-p", "fleximark-render-html", "-p", "fleximark-engine", "-p", "fleximark-language", "-p", "fleximark-application")


def vscode_task(_: Sequence[str]) -> None:
    build_vscode_product(build_runtime())


def vscode_prepublish(args: Sequence[str]) -> None:
    if args:
        raise RuntimeError("vscode-prepublish does not accept arguments")
    if os.environ.get("FLEXIMARK_RELEASE_PREBUILT") != "1":
        check_protocol_contract()
        vscode_task(())
        return

    validate_prebuilt_inputs(Path.cwd())


WatchProcess = subprocess.Popen[str]


def stop_processes(processes: Sequence[WatchProcess]) -> None:
    for process in processes:
        if process.poll() is None:
            process.terminate()
    for process in processes:
        if process.poll() is not None:
            continue
        try:
            process.wait(timeout=5)
        except subprocess.TimeoutExpired:
            process.kill()
            process.wait(timeout=5)


def start_watch_process(
    command: Sequence[str], ready_pattern: str
) -> tuple[WatchProcess, threading.Event]:
    ready = threading.Event()
    process = subprocess.Popen(
        [executable(command[0]), *command[1:]],
        cwd=ROOT,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        text=True,
        encoding="utf-8",
        errors="replace",
        bufsize=1,
    )

    def forward_output() -> None:
        if process.stdout is None:
            return
        for line in process.stdout:
            print(line, end="", flush=True)
            if ready_pattern in line:
                ready.set()

    threading.Thread(target=forward_output, daemon=True).start()
    return process, ready


def watch_vscode() -> None:
    source = ROOT / ".artifacts" / metadata()["version"] / host_target() / "vscode"
    development = ROOT / ".build" / "vscode" / "product"
    shutil.copytree(source, development, dirs_exist_ok=True)
    commands = [
        (command, "[watch] build finished")
        for command in javascript_build.watch_commands()
    ]
    commands.append(
        (["yarn", "exec", "tsc", "-b", "--watch"], "Watching for file changes.")
    )
    processes: list[WatchProcess] = []
    readiness: list[threading.Event] = []
    try:
        print("FlexiMark VS Code watch starting", flush=True)
        for command, ready_pattern in commands:
            process, ready = start_watch_process(command, ready_pattern)
            processes.append(process)
            readiness.append(ready)
        while not all(ready.is_set() for ready in readiness):
            for process in processes:
                code = process.poll()
                if code is not None:
                    raise subprocess.CalledProcessError(code, process.args)
            time.sleep(0.05)
        refresh_development_resources(development)
        print("FlexiMark VS Code watch ready", flush=True)
        print("Watching JavaScript/TypeScript only. For Rust changes, stop the watch task and restart F5. Ctrl+C stops watching.", flush=True)
        while True:
            refresh_development_resources(development)
            for process in processes:
                code = process.poll()
                if code is None:
                    continue
                if code != 0:
                    raise subprocess.CalledProcessError(code, process.args)
                return
            time.sleep(0.1)
    except KeyboardInterrupt:
        pass
    finally:
        stop_processes(processes)


def refresh_development_resources(product: Path) -> None:
    from distribution import digest
    runtime = product / "runtime"
    manifest_path = runtime / "manifest.json"
    manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
    files = {name: digest(runtime / name) for name in manifest["files"]}
    if files != manifest["files"]:
        manifest["files"] = files
        manifest_path.write_text(json.dumps(manifest, indent=2) + "\n", encoding="utf-8")


def compile_tests() -> None:
    javascript_build.build_electron_tests()


def run_editor_integration() -> None:
    yarn("vscode-test", "--config", "adapters/vscode/.vscode-test.mjs")
    run("node", "tooling/tasks/restricted-vscode.mjs")


def integration_test(args: Sequence[str]) -> None:
    arguments = tuple(args)
    if arguments not in {(), ("--prebuilt",)}:
        raise RuntimeError("test accepts only the optional --prebuilt flag")
    check_protocol_contract()
    compile_tests()
    if not arguments:
        vscode_task(())
    run_editor_integration()


def pure_test(_: Sequence[str]) -> None:
    check_protocol_contract()
    javascript_build.build_pure_tests()
    run("node", "--test", ".build/tests/unit/pure-tests.cjs")


def python_test() -> None:
    run(
        "python",
        "-m",
        "unittest",
        "discover",
        "-s",
        "tooling/tasks/tests",
        "-p",
        "test_*.py",
    )


def localization(_: Sequence[str]) -> None:
    export_localization()


def check_localization() -> None:
    check_generated_freshness(lambda: localization(()), [ROOT / "adapters/vscode/l10n"])


def check_generated_freshness(generate: Callable[[], None], paths: Sequence[Path]) -> None:
    """Check regeneration against the working tree, including added/deleted files."""
    def snapshot() -> dict[Path, bytes]:
        files: dict[Path, bytes] = {}
        for path in paths:
            candidates = path.rglob("*") if path.is_dir() else [path]
            files.update((candidate, candidate.read_bytes()) for candidate in candidates if candidate.is_file())
        return files

    before = snapshot()
    generate()
    after = snapshot()
    changed = sorted(str(path.relative_to(ROOT) if path.is_relative_to(ROOT) else path) for path in before.keys() | after.keys() if before.get(path) != after.get(path))
    if changed:
        raise RuntimeError("generated files were stale; review regenerated output: " + ", ".join(changed))


def check_architecture() -> None:
    run("cargo", "xtask", "check")
    run("cargo", "xtask", "check", "--root", "adapters/zed")


def verify_audit(_: Sequence[str]) -> None:
    """Network dependency audit required explicitly by CI and release workflows."""
    run("yarn", "npm", "audit", "--all", "--recursive", "--severity", "high")


def verify_built_product(package_args: Sequence[str] = ()) -> None:
    """Host gates that require the assembled native and editor product."""
    check_performance_budgets()
    compile_tests()
    run_editor_integration()
    vsix = package_built_product(package_args)
    from release_artifact import create_identity, verify_identity
    source_head = os.environ.get("SOURCE_GIT_HEAD") or subprocess.check_output(
        [executable("git"), "rev-parse", "HEAD"], cwd=ROOT, text=True
    ).strip()
    tag = f'v{metadata()["version"]}'
    identity = vsix.with_suffix(".vsix.identity.json")
    create_identity(vsix, identity, expected_version=metadata()["version"], git_tag=tag, source_git_head=source_head)
    verify_identity(vsix, identity, expected_git_tag=tag, expected_source_git_head=source_head)
    smoke_vsix(vsix)


def package_built_product(args: Sequence[str]) -> Path:
    target = host_target()
    output = ROOT / ".artifacts" / metadata()["version"] / target
    run("node", str(ROOT / "node_modules" / "@vscode" / "vsce" / "vsce"), "package", "--no-dependencies", "--target", next(name for name, rust in __import__("distribution").TARGETS.items() if rust == target), "--out", str(output / f'fleximark-{metadata()["version"]}-{target}.vsix'), *args, cwd=output / "vscode")

    return output / f'fleximark-{metadata()["version"]}-{target}.vsix'


@dataclass(frozen=True)
class Step:
    description: str
    action: Callable[[], object]


@dataclass(frozen=True)
class Plan:
    steps: Sequence[Step]
    result: str

    def execute(self, *, dry_run: bool = False) -> None:
        print("Execution plan (no steps will run):" if dry_run else "Execution plan:", flush=True)
        for index, step in enumerate(self.steps, 1):
            print(f"  {index}. {step.description}", flush=True)
        print(f"On success: {self.result}", flush=True)
        if dry_run:
            return
        for index, step in enumerate(self.steps, 1):
            print(f"\n[{index}/{len(self.steps)}] {step.description}", flush=True)
            step.action()
        print(f"\nCompleted: {self.result}", flush=True)


def runtime_archive() -> Path:
    return ROOT / ".artifacts" / metadata()["version"] / host_target() / f'fleximark-{metadata()["version"]}-{host_target()}.zip'


def runtime_build_steps() -> list[Step]:
    return [
        Step("Synchronize product manifests from release.toml", generate_versions),
        Step("Build native daemon and CLI (release, current platform)", build_daemon),
        Step("Build browser preview JavaScript and copy its CSS", javascript_build.build_browser_client),
        Step("Build browser workspace JavaScript and copy its CSS", javascript_build.build_workspace_ui),
        Step("Assemble runtime directory, ZIP and SHA-256 checksum", lambda: assemble_runtime(host_target())),
    ]


def build_steps(target: str) -> list[Step]:
    steps = [Step("Check generated protocol against the Rust contract", check_protocol_contract)]
    if target == "core":
        return steps + [Step("Build Rust document and application libraries (debug)", lambda: build_core(()))]
    steps += runtime_build_steps()
    if target in {"vscode", "all"}:
        steps.append(Step("Build and stage the VS Code extension with that runtime", lambda: build_vscode_product(runtime_archive())))
    if target in {"zed", "all"}:
        steps.append(Step("Build Zed WASM extension; stage it and create ZIP/checksum", build_zed_product))
    return steps


def check_steps() -> list[Step]:
    return [
        Step("Check product version freshness against release.toml", lambda: check_generated_freshness(generate_versions, [ROOT / name for name in ("adapters/vscode/package.json", "adapters/zed/extension.toml", "apps/server/Cargo.toml", "apps/cli/Cargo.toml", "Cargo.lock")])),
        Step("Check generated protocol against the Rust contract", check_protocol_contract),
        Step("Check root/Zed Rust architecture and diagram freshness", lambda: check_generated_freshness(check_architecture, [ROOT / name for name in ("architecture", "adapters/zed/architecture")])),
        Step("Check Python task syntax", lambda: run("python", "-m", "compileall", "-q", "tooling/tasks")),
        Step("Check all TypeScript project types", lambda: yarn("tsc", "-b")),
        Step("Lint TypeScript/JavaScript sources", lambda: yarn("eslint", "adapters/vscode", "web", "contracts/typescript", "tests/typescript", "tooling/tasks")),
        Step("Check generated English localization freshness", check_localization),
        Step("Check root Rust formatting", lambda: run("cargo", "fmt", "--all", "--", "--check")),
        Step("Check Zed Rust formatting", lambda: run("cargo", "fmt", "--manifest-path", "adapters/zed/Cargo.toml", "--", "--check")),
        Step("Lint Zed Rust code and test targets", lambda: run("cargo", "clippy", "--manifest-path", "adapters/zed/Cargo.toml", "--target-dir", "target/zed", "--all-targets", "--locked", "--", "-D", "warnings")),
        Step("Lint root Rust workspace code and test targets", lambda: run("cargo", "clippy", "--workspace", "--all-targets", "--locked", "--", "-D", "warnings")),
    ]


def unit_steps() -> list[Step]:
    return [
        Step("Run Python and Node build/release tooling tests", python_test),
        Step("Run root Rust workspace tests (including native integration)", lambda: run("cargo", "test", "--workspace", "--all-targets", "--locked")),
        Step("Run native Zed adapter tests", lambda: run("cargo", "test", "--manifest-path", "adapters/zed/Cargo.toml", "--target-dir", "target/zed", "--locked")),
        Step("Check protocol; compile and run Node protocol/preview/adapter tests", lambda: pure_test(())),
    ]


def create_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        description="FlexiMark development commands. Use mise run <command> [arguments].",
        epilog="Every command supports --dry-run. Build/package require an explicit target. See docs/DEVELOPMENT.md.",
    )
    commands = parser.add_subparsers(dest="command", metavar="command")

    def command(name: str, help_text: str) -> argparse.ArgumentParser:
        subparser = commands.add_parser(name, help=help_text, description=help_text)
        subparser.add_argument("--dry-run", action="store_true", help="print the ordered steps without executing them")
        return subparser

    command("setup", "Install locked dependencies, Rust components, WASM target and pinned submodules")
    command("watch", "Build only the VS Code product, then watch JS/TS; does not launch an editor or watch Rust")
    command("build", "Build the selected product; no tests or editor launch").add_argument("target", choices=("core", "runtime", "vscode", "zed", "all"))
    command("check", "Check generated files, architecture, formatting, types and lint; no test suites")
    command("test", "Run tests; default is unit (Python + Rust + Node, without GUI)").add_argument("suite", nargs="?", default="unit", choices=("unit", "editor"), help="editor builds the VS Code product and launches integration test hosts")
    command("package", "Build and package the selected product; no tests, smoke test or publication").add_argument("target", choices=("runtime", "vscode", "zed", "all"))
    command("verify", "Run all checks, unit tests, network audit, all builds, performance/editor tests, VSIX packaging and clean-install smoke")
    command("audit", "Query the package registry for high/critical JavaScript dependency vulnerabilities (network)")
    command("smoke", "Install and test an existing VSIX in an isolated VS Code profile").add_argument("vsix", type=Path)
    command("generate", "Update the selected tracked generated files").add_argument("target", choices=("protocol", "l10n", "versions", "architecture", "all"))
    command("clean", "Delete generated JS/TS and test output in .build; preserve target and .artifacts")
    command("ci", "Internal CI phases; uses the same implementation as local commands").add_argument("phase", choices=("quality", "product", "zed"))
    command("vscode-prepublish", "Internal VS Code packaging callback")
    return parser


def create_plan(args: argparse.Namespace) -> Plan:
    command = args.command
    artifacts = f".artifacts/{metadata()['version']}/{host_target()}"
    if command == "setup":
        return Plan([
            Step("Initialize/update pinned Git submodules", lambda: run("git", "submodule", "update", "--init", "--recursive")),
            Step("Install locked JavaScript dependencies", lambda: run("yarn", "install", "--immutable")),
            Step("Install locked Python environment", lambda: run("uv", "sync", "--frozen")),
            Step("Install Rust formatting and lint components", lambda: run("rustup", "component", "add", "rustfmt", "clippy")),
            Step("Install WASI target for Zed", lambda: run("rustup", "target", "add", "wasm32-wasip2")),
        ], "Development dependencies ready. Toolchain versions are installed first with mise install.")
    if command in {"build", "package"}:
        steps = build_steps(args.target)
        if command == "package" and args.target in {"vscode", "all"}:
            steps.append(Step("Create platform VSIX from the assembled VS Code product", lambda: package_built_product(())))
        runtime_zip = f"{artifacts}/fleximark-{metadata()['version']}-{host_target()}.zip"
        zed_zip = f"{artifacts}/fleximark-zed-{metadata()['version']}.zip"
        vsix = f"{artifacts}/fleximark-{metadata()['version']}-{host_target()}.vsix"
        result = {
            "core": "Rust libraries in target/debug/",
            "runtime": f"{artifacts}/runtime/ and {runtime_zip} (+ .sha256)",
            "vscode": f"{artifacts}/vscode/ and {runtime_zip} (+ .sha256). To create a VSIX: mise run package vscode",
            "zed": f"{artifacts}/zed/, {zed_zip} and {runtime_zip} (+ .sha256)",
            "all": f"{artifacts}/runtime/, vscode/, zed/ and runtime/Zed ZIPs (+ .sha256). To create a VSIX too: mise run package all",
        }[args.target]
        if command == "package" and args.target in {"vscode", "all"}:
            result = vsix if args.target == "vscode" else f"{vsix}, {runtime_zip} and {zed_zip} (+ .sha256 for ZIPs)"
        return Plan(steps, result)
    if command == "watch":
        return Plan([*build_steps("vscode"), Step("Copy VS Code staging to .build/vscode/product; watch JavaScript/TypeScript until Ctrl+C (no editor launch)", watch_vscode)], ".build/vscode/product; F5 launches the extension. Rust/CSS changes require stopping watch and restarting F5.")
    if command == "check":
        return Plan(check_steps(), "Source checks passed. Stale generated files are regenerated and reported as a failure.")
    if command == "test":
        if args.suite == "editor":
            return Plan([*build_steps("vscode"), Step("Compile and run VS Code workspace, standalone and restricted-workspace integration tests", lambda: integration_test(("--prebuilt",)))], "VS Code integration tests passed (GUI; may download VS Code).")
        return Plan(unit_steps(), "All unit/native tests passed; no editor/browser launched.")
    if command == "verify":
        return Plan([
            Step("Enable real browser acceptance tests unless explicitly configured", lambda: os.environ.setdefault("FLEXIMARK_BROWSER_DOM", "true")),
            *check_steps(),
            *unit_steps(),
            Step("Audit JavaScript dependencies (network)", lambda: verify_audit(())),
            *build_steps("all"),
            Step("Check performance, run VS Code integration, create VSIX, verify identity and smoke-test clean installation", verify_built_product),
        ], f"All local release gates passed; packages in {artifacts}. Requires GUI/browser and network; publishes nothing.")
    if command == "audit":
        return Plan([Step("Run yarn npm audit for high/critical vulnerabilities", lambda: verify_audit(()))], "Dependency audit passed.")
    if command == "smoke":
        return Plan([Step(f"Clean-install and smoke-test {args.vsix}", lambda: smoke_vsix(args.vsix))], "Existing VSIX passed clean-install smoke test (GUI).")
    if command == "generate":
        generators = {
            "protocol": Step("Regenerate protocol schema, TypeScript contract and fixtures", generate_protocol_contract),
            "l10n": Step("Regenerate English localization bundle", export_localization),
            "versions": Step("Synchronize product manifests from release.toml", generate_versions),
            "architecture": Step("Check all Rust architectures and regenerate diagrams", check_architecture),
        }
        steps = list(generators.values()) if args.target == "all" else [generators[args.target]]
        return Plan(steps, "Generated files updated; review and commit their changes with the source.")
    if command == "clean":
        return Plan([Step("Remove .build/vscode, preview, workspace-ui, types and tests", lambda: clean(()))], "JS/TS and test output removed. Rust target and distribution artifacts preserved.")
    if command == "vscode-prepublish":
        return Plan([Step("Validate release prebuilt inputs or build only the VS Code product", lambda: vscode_prepublish(()))], "VS Code prepublish callback complete.")
    phases = {
        "quality": [*check_steps(), *unit_steps()],
        "product": [*build_steps("vscode"), Step("Check performance, run editor integration, create VSIX, verify identity and smoke-test clean installation", verify_built_product)],
        "zed": [Step("Synchronize product manifests from release.toml", generate_versions), Step("Build only Zed WASM extension and create ZIP/checksum", build_zed_product)],
    }
    return Plan(phases[args.phase], f"CI {args.phase} phase passed.")


def main(argv: Sequence[str] | None = None) -> None:
    parser = create_parser()
    args = parser.parse_args(argv)
    if args.command is None:
        parser.print_help()
        return
    create_plan(args).execute(dry_run=args.dry_run)


if __name__ == "__main__":
    script_entrypoint(main)
