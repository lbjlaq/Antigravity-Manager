export function isTauriEnvironment(): boolean {
  if (typeof window === "undefined") return false;

  const w = window as unknown as Record<string, unknown>;

  if ("__TAURI_INTERNALS__" in w) return true;
  if ("__TAURI__" in w) return true;
  if ("__TAURI_INVOKE__" in w) return true;

  const userAgent = typeof navigator !== "undefined" ? navigator.userAgent : "";
  if (typeof userAgent === "string" && userAgent.toLowerCase().includes("tauri")) return true;

  return false;
}

