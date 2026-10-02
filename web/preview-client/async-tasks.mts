export class AsyncTaskObserver {
  observe(task: Promise<unknown>): void {
    void task.catch(() => undefined);
  }
}
