from __future__ import annotations
import json
import shutil
import subprocess
import threading
import time
from pathlib import Path
from collections.abc import Sequence
import build as javascript_build
from _tools import ROOT, executable
from distribution import host_target, metadata, digest

def stop_processes(processes: Sequence[subprocess.Popen[str]]) -> None:
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

def start_watch_process(command: Sequence[str], ready_pattern: str) -> tuple[subprocess.Popen[str], threading.Event]:
    ready = threading.Event()
    process = subprocess.Popen(
        [executable(command[0]), *command[1:]],
        cwd=ROOT,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        text=True,
        encoding='utf-8',
        errors='replace',
        bufsize=1,
    )

    def forward_output() -> None:
        if process.stdout is None:
            return
        for line in process.stdout:
            print(line, end='', flush=True)
            if ready_pattern in line:
                ready.set()
    threading.Thread(target=forward_output, daemon=True).start()
    return (process, ready)

def watch_vscode() -> None:
    source = ROOT / '.artifacts' / metadata()['version'] / host_target() / 'vscode'
    development = ROOT / '.build' / 'vscode' / 'product'
    shutil.copytree(source, development, dirs_exist_ok=True)
    commands = [(command, '[watch] build finished') for command in javascript_build.watch_commands()]
    commands.append((['yarn', 'exec', 'tsc', '-b', '--watch'], 'Watching for file changes.'))
    memo: dict[str, tuple[int, int, str]] = {}
    processes: list[subprocess.Popen[str]] = []
    readiness: list[threading.Event] = []
    try:
        print('FlexiMark VS Code watch starting', flush=True)
        for command, ready_pattern in commands:
            process, ready = start_watch_process(command, ready_pattern)
            processes.append(process)
            readiness.append(ready)
        while not all((ready.is_set() for ready in readiness)):
            for process in processes:
                code = process.poll()
                if code is not None:
                    raise subprocess.CalledProcessError(code, process.args)
            time.sleep(0.05)
        refresh_development_resources(development, memo)
        print('FlexiMark VS Code watch ready', flush=True)
        print(
            'Watching JavaScript/TypeScript only. For Rust changes, stop the watch task and restart F5. Ctrl+C stops watching.',
            flush=True,
        )
        while True:
            refresh_development_resources(development, memo)
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

def refresh_development_resources(product: Path, memo: dict[str, tuple[int, int, str]]) -> None:
    runtime = product / 'runtime'
    manifest_path = runtime / 'manifest.json'
    manifest = json.loads(manifest_path.read_text(encoding='utf-8'))
    files = {}
    for name in manifest['files']:
        path = runtime / name
        metadata = path.stat()
        signature = (metadata.st_mtime_ns, metadata.st_size)
        previous = memo.get(name)
        checksum = previous[2] if previous is not None and previous[:2] == signature else digest(path)
        memo[name] = (*signature, checksum)
        files[name] = checksum
    for removed in memo.keys() - files.keys():
        del memo[removed]
    if files != manifest['files']:
        manifest['files'] = files
        manifest_path.write_text(json.dumps(manifest, indent=2) + '\n', encoding='utf-8')
