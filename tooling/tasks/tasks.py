from __future__ import annotations

import argparse
import os
import shutil
import subprocess
import threading
import time
from collections.abc import Sequence
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
    generate_versions()
    build_daemon()
    javascript_build.build_browser_client()
    javascript_build.build_workspace_ui()
    return assemble_runtime(host_target())


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


def build_product() -> None:
    archive = build_runtime()
    build_vscode_product(archive)
    build_zed_product()


def build_core(_: Sequence[str]) -> None:
    run("cargo", "build", "--locked", "-p", "fleximark-model", "-p", "fleximark-parser", "-p", "fleximark-render-html", "-p", "fleximark-engine", "-p", "fleximark-language", "-p", "fleximark-plugin-api", "-p", "fleximark-application")


def runtime_task(_: Sequence[str]) -> None:
    build_runtime()


def vscode_task(_: Sequence[str]) -> None:
    build_vscode_product(build_runtime())


def zed_task(_: Sequence[str]) -> None:
    build_runtime()
    build_zed_product()


def stage_zed_task(_: Sequence[str]) -> None:
    generate_versions()
    build_zed_product()


def build(_: Sequence[str]) -> None:
    check_protocol_contract()
    build_product()


def vscode_prepublish(args: Sequence[str]) -> None:
    if args:
        raise RuntimeError("vscode-prepublish does not accept arguments")
    if os.environ.get("FLEXIMARK_RELEASE_PREBUILT") != "1":
        build(())
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


def dev(_: Sequence[str]) -> None:
    check_protocol_contract()
    build_product()
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
        print("FlexiMark development build starting", flush=True)
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
        print("FlexiMark development build ready", flush=True)
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
        build_product()
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
    run("node", "--test", "tooling/tasks/tests/semantic_release_package.test.mjs")


def localization(_: Sequence[str]) -> None:
    export_localization()


def check_localization() -> None:
    localization(())
    run("git", "diff", "--exit-code", "--", "adapters/vscode/l10n")


def verify(_: Sequence[str]) -> None:
    check_protocol_contract()
    run("cargo", "xtask", "check")
    run("cargo", "xtask", "check", "--root", "adapters/zed")
    run("cargo", "xtask", "check", "--root", "tests/fixtures/plugin-component")
    run("git", "diff", "--exit-code", "--", "architecture", "adapters/zed/architecture", "tests/fixtures/plugin-component/architecture")
    run("python", "-m", "compileall", "-q", "tooling/tasks")
    python_test()
    yarn("tsc", "-b")
    yarn("eslint", "adapters/vscode", "web", "contracts/typescript", "tests/typescript", "tooling/tasks")
    check_localization()
    build_product()
    compile_tests()
    javascript_build.build_pure_tests()
    run("node", "--test", ".build/tests/unit/pure-tests.cjs")
    run("node", str(ROOT / "node_modules" / "@vscode" / "vsce" / "vsce"), "ls", "--no-dependencies", cwd=ROOT / ".artifacts" / metadata()["version"] / host_target() / "vscode")
    run("cargo", "fmt", "--all", "--", "--check")
    run("cargo", "fmt", "--manifest-path", "adapters/zed/Cargo.toml", "--", "--check")
    run("cargo", "test", "--manifest-path", "adapters/zed/Cargo.toml", "--target-dir", "target/zed", "--locked")
    run("cargo", "clippy", "--manifest-path", "adapters/zed/Cargo.toml", "--target-dir", "target/zed", "--all-targets", "--locked", "--", "-D", "warnings")
    run("cargo", "test", "--manifest-path", "tests/fixtures/plugin-component/Cargo.toml", "--target-dir", "target/fixture-native", "--locked")
    run("cargo", "fmt", "--manifest-path", "tests/fixtures/plugin-component/Cargo.toml", "--", "--check")
    run("cargo", "clippy", "--manifest-path", "tests/fixtures/plugin-component/Cargo.toml", "--target-dir", "target/fixture-native", "--all-targets", "--locked", "--", "-D", "warnings")
    run("cargo", "check", "--manifest-path", "tests/fixtures/plugin-component/Cargo.toml", "--target", "wasm32-wasip2", "--target-dir", "target/fixture-wasm", "--locked")
    run("cargo", "test", "--workspace", "--all-targets", "--locked")
    run(
        "cargo",
        "clippy",
        "--workspace",
        "--all-targets",
        "--locked",
        "--",
        "-D",
        "warnings",
    )
    check_performance_budgets()
    run_editor_integration()


def package_vsix(args: Sequence[str]) -> None:
    verify(())
    target = host_target()
    output = ROOT / ".artifacts" / metadata()["version"] / target
    run("node", str(ROOT / "node_modules" / "@vscode" / "vsce" / "vsce"), "package", "--no-dependencies", "--target", next(name for name, rust in __import__("distribution").TARGETS.items() if rust == target), "--out", str(output / f'fleximark-{metadata()["version"]}-{target}.vsix'), *args, cwd=output / "vscode")


def smoke(args: Sequence[str]) -> None:
    if len(args) != 1:
        raise RuntimeError("smoke requires exactly one VSIX path")
    smoke_vsix(Path(args[0]))


TASKS = {
    "build": build,
    "build-all": build,
    "build-core": build_core,
    "build-runtime": runtime_task,
    "build-vscode": vscode_task,
    "build-zed": zed_task,
    "stage-zed": stage_zed_task,
    "clean": clean,
    "dev": dev,
    "l10n": localization,
    "package": package_vsix,
    "protocol-check": check_protocol_contract,
    "protocol-generate": generate_protocol_contract,
    "test-pure": pure_test,
    "smoke": smoke,
    "test": integration_test,
    "verify": verify,
    "vscode-prepublish": vscode_prepublish,
}


def main() -> None:
    parser = argparse.ArgumentParser(description="Run FlexiMark development tasks")
    parser.add_argument("task", choices=TASKS)
    parser.add_argument("task_args", nargs=argparse.REMAINDER)
    args = parser.parse_args()
    TASKS[args.task](args.task_args)


if __name__ == "__main__":
    script_entrypoint(main)
