/** Small, versioned local view state. Invalid or unavailable storage falls back cleanly. */
export function readViewState<T>(key: string, fallback: T): T {
  try {
    const value = localStorage.getItem(`tenjee-vault:${key}`);
    return value === null ? fallback : JSON.parse(value) as T;
  } catch {
    return fallback;
  }
}

export function writeViewState(key: string, value: unknown): void {
  try {
    localStorage.setItem(`tenjee-vault:${key}`, JSON.stringify(value));
  } catch {
    // View restoration is best-effort when browser storage is unavailable or full.
  }
}
