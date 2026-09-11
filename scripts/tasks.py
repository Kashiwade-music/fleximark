from __future__ import annotations

import argparse
import subprocess
import time
from collections.abc import Sequence
from pathlib import Path

import build as javascript_build
from _tools import ROOT, executable, run, script_entrypoint, yarn
from check_performance_budgets import check_performance_budgets
from create_release_manifest import create_manifest
from l10n_export import export_localization
from smoke_vsix import smoke_vsix
from stage_daemon import stage_daemon
from verify_architecture import verify_architecture


def clean(_: Sequence[str]) -> None:
    javascript_build.clean()


def build(_: Sequence[str]) -> None:
    javascript_build.build_browser_client()
    run("cargo", "build", "--release", "-p", "fleximarkd")
    stage_daemon()
    create_manifest()
    javascript_build.build_extension(production=True)


def stop_processes(processes: Sequence[subprocess.Popen[bytes]]) -> None:
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


def dev(_: Sequence[str]) -> None:
    javascript_build.clean()
    commands = javascript_build.watch_commands()
    commands.append(
        ["yarn", "exec", "tsc", "--noEmit", "--watch", "--project", "tsconfig.json"]
    )
    processes: list[subprocess.Popen[bytes]] = []
    try:
        for command in commands:
            processes.append(
                subprocess.Popen([executable(command[0]), *command[1:]], cwd=ROOT)
            )
        while True:
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


def compile_tests() -> None:
    javascript_build.build_tests()


def integration_test(args: Sequence[str]) -> None:
    arguments = tuple(args)
    if arguments not in {(), ("--prebuilt",)}:
        raise RuntimeError("test accepts only the optional --prebuilt flag")
    compile_tests()
    if not arguments:
        build(())
    yarn("vscode-test")


def pure_test(_: Sequence[str]) -> None:
    javascript_build.build_pure_tests()
    run("node", "--test", "out/test/pure-tests.cjs")


def python_test() -> None:
    run(
        "python",
        "-m",
        "unittest",
        "discover",
        "-s",
        "scripts/tests",
        "-p",
        "test_*.py",
    )


def localization(_: Sequence[str]) -> None:
    export_localization()


def check_localization() -> None:
    localization(())
    run("git", "diff", "--exit-code", "--", "l10n")


def verify(_: Sequence[str]) -> None:
    verify_architecture()
    run("python", "-m", "compileall", "-q", "scripts")
    python_test()
    yarn("tsc", "--noEmit")
    yarn("eslint", "adapters", "web", "test", "scripts")
    check_localization()
    build(())
    compile_tests()
    run("node", "--test", "out/test/pure-tests.cjs")
    yarn("vsce", "ls", "--no-dependencies")
    run("cargo", "fmt", "--all", "--", "--check")
    run("cargo", "test", "--workspace", "--all-targets")
    run("cargo", "clippy", "--workspace", "--all-targets", "--", "-D", "warnings")
    check_performance_budgets()
    yarn("vscode-test")


def package_vsix(args: Sequence[str]) -> None:
    verify(())
    yarn("vsce", "package", "--no-dependencies", *args)


def smoke(args: Sequence[str]) -> None:
    if len(args) != 1:
        raise RuntimeError("smoke requires exactly one VSIX path")
    smoke_vsix(Path(args[0]))


TASKS = {
    "build": build,
    "clean": clean,
    "dev": dev,
    "l10n": localization,
    "package": package_vsix,
    "test-pure": pure_test,
    "smoke": smoke,
    "test": integration_test,
    "verify": verify,
}


def main() -> None:
    parser = argparse.ArgumentParser(description="Run FlexiMark development tasks")
    parser.add_argument("task", choices=TASKS)
    parser.add_argument("task_args", nargs=argparse.REMAINDER)
    args = parser.parse_args()
    TASKS[args.task](args.task_args)


if __name__ == "__main__":
    script_entrypoint(main)
