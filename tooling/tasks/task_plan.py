from __future__ import annotations
from collections.abc import Callable, Sequence
from dataclasses import dataclass

@dataclass(frozen=True)
class Step:
    description: str
    action: Callable[[], object]

@dataclass(frozen=True)
class Plan:
    steps: Sequence[Step]
    result: str

    def execute(self, *, dry_run: bool=False) -> None:
        print('Execution plan (no steps will run):' if dry_run else 'Execution plan:', flush=True)
        for index, step in enumerate(self.steps, 1):
            print(f'  {index}. {step.description}', flush=True)
        print(f'On success: {self.result}', flush=True)
        if dry_run:
            return
        for index, step in enumerate(self.steps, 1):
            print(f'\n[{index}/{len(self.steps)}] {step.description}', flush=True)
            step.action()
        print(f'\nCompleted: {self.result}', flush=True)
