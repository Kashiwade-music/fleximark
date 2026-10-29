from __future__ import annotations
from collections.abc import Sequence
import build as javascript_build
from _tools import run, yarn

def clean(_: Sequence[str]) -> None:
    javascript_build.clean()

def compile_tests() -> None:
    javascript_build.build_electron_tests()

def run_editor_integration() -> None:
    yarn('vscode-test', '--config', 'adapters/vscode/.vscode-test.mjs')
    run('node', 'tooling/tasks/restricted-vscode.mjs')

def pure_test(_: Sequence[str]) -> None:
    javascript_build.build_pure_tests()
    run('node', '--test', '.build/tests/unit/pure-tests.cjs')

def python_test() -> None:
    run('python', '-m', 'unittest', 'discover', '-s', 'tooling/tasks/tests', '-p', 'test_*.py')
