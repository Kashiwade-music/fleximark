/** Waits for observable asynchronous behavior with a bounded deadline. */
export async function pollUntil<T>(
  read: () => T | Promise<T>,
  complete: (value: T) => boolean,
  label: string,
  timeoutMilliseconds = 10_000,
): Promise<T> {
  const deadline = Date.now() + timeoutMilliseconds;
  for (;;) {
    const value = await read();
    if (complete(value)) return value;
    if (Date.now() >= deadline)
      throw new Error(`Timed out waiting for ${label}`);
    await new Promise((resolve) => setTimeout(resolve, 25));
  }
}

export async function until(
  read: () => boolean | Promise<boolean>,
  label: string,
  timeoutMilliseconds = 10_000,
): Promise<void> {
  await pollUntil(read, Boolean, label, timeoutMilliseconds);
}
