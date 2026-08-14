/** Extract a human-readable message from a Tauri invoke rejection or any thrown value. */
export function getErrorMessage(err: unknown, fallback = "Unknown error"): string {
  if (err instanceof Error) {
    return err.message;
  }
  if (typeof err === "string") {
    return err;
  }
  if (err && typeof err === "object") {
    const record = err as Record<string, unknown>;
    if (typeof record.message === "string") {
      return record.message;
    }
    if (typeof record.error === "string") {
      return record.error;
    }
    try {
      return JSON.stringify(record);
    } catch {
      // Fall through to the generic message below.
    }
  }
  return fallback;
}

/** Detect authentication/credential-expiry errors returned by the backend. */
export function isAuthError(err: unknown): boolean {
  if (err && typeof err === "object") {
    const record = err as Record<string, unknown>;
    if (record.type === "Auth") return true;
  }

  const message = getErrorMessage(err).toLowerCase();
  return (
    message.includes("token expired") ||
    message.includes("please re-login") ||
    message.includes("please login") ||
    message.includes("no active session")
  );
}
