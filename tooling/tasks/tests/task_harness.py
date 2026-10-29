"""Capture task actions at their owning module without invoking subprocesses."""
from contextlib import ExitStack, contextmanager
from unittest.mock import Mock, patch

import tasks
import task_checks
import task_js
import task_plan
import task_products
import task_watch
import task_workflows

OWNERS = (tasks, task_checks, task_js, task_plan, task_products, task_watch, task_workflows)


@contextmanager
def action(name, **options):
    mock = Mock(**options)
    targets = [owner for owner in OWNERS if hasattr(owner, name)]
    if not targets:
        raise AssertionError(f"no task owns {name}")
    with ExitStack() as stack:
        for owner in targets:
            stack.enter_context(patch.object(owner, name, mock))
        yield mock
