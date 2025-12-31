import { invoke } from "@tauri-apps/api/core";

type FrontendLogLevel = "error" | "warn" | "info";

function safeStringify(value: unknown): string {
  if (typeof value === "string") return value;
  try {
    return JSON.stringify(value);
  } catch {
    return String(value);
  }
}

let lastSignature = "";
let lastAt = 0;

async function send(level: FrontendLogLevel, message: string, stack?: string) {
  const now = Date.now();
  const signature = `${level}:${message}:${stack ?? ""}`;

  // Deduplicate bursts (common for render loops).
  if (signature === lastSignature && now - lastAt < 1000) return;
  lastSignature = signature;
  lastAt = now;

  try {
    await invoke("frontend_log", { level, message, stack });
  } catch {
    // Swallow to avoid infinite loops if invoke fails.
  }
}

export function initFrontendLogging() {
  const originalError = console.error.bind(console);
  const originalWarn = console.warn.bind(console);
  const originalInfo = console.info.bind(console);

  console.error = (...args: unknown[]) => {
    originalError(...args);
    const msg = args.map(safeStringify).join(" ");
    void send("error", msg);
  };

  console.warn = (...args: unknown[]) => {
    originalWarn(...args);
    const msg = args.map(safeStringify).join(" ");
    void send("warn", msg);
  };

  console.info = (...args: unknown[]) => {
    originalInfo(...args);
    const msg = args.map(safeStringify).join(" ");
    void send("info", msg);
  };

  window.addEventListener("error", (event) => {
    const message = event.message || safeStringify(event.error) || "Unknown window error";
    const stack = (event.error as Error | undefined)?.stack;
    void send("error", message, stack);
  });

  window.addEventListener("unhandledrejection", (event) => {
    const reason = (event as PromiseRejectionEvent).reason;
    const message =
      reason instanceof Error ? reason.message : safeStringify(reason) || "Unhandled rejection";
    const stack = reason instanceof Error ? reason.stack : undefined;
    void send("error", `Unhandled promise rejection: ${message}`, stack);
  });

  // Keep a light breadcrumb in case the UI is blank with no further events.
  void send("info", "Frontend initialized");
}
