export function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason?: unknown) => void;
  const promise = new Promise<T>((accept, decline) => {
    resolve = accept;
    reject = decline;
  });
  return { promise, resolve, reject };
}

export function signal() {
  const value = deferred<undefined>();
  return {
    promise: value.promise,
    resolve: () => value.resolve(undefined),
    reject: value.reject,
  };
}

export async function flushMicrotasks(turns = 3): Promise<void> {
  while (turns-- > 0) await Promise.resolve();
}
