import React from "react";
import { invoke } from "@tauri-apps/api/core";

type Props = {
  children: React.ReactNode;
};

type State = {
  hasError: boolean;
  message?: string;
};

export class ErrorBoundary extends React.Component<Props, State> {
  state: State = { hasError: false };

  static getDerivedStateFromError(error: unknown): State {
    return { hasError: true, message: error instanceof Error ? error.message : String(error) };
  }

  async componentDidCatch(error: unknown) {
    try {
      const message = error instanceof Error ? error.message : String(error);
      const stack = error instanceof Error ? error.stack : undefined;
      await invoke("frontend_log", { level: "error", message: `React error boundary: ${message}`, stack });
    } catch {
      // ignore
    }
  }

  render() {
    if (!this.state.hasError) return this.props.children;

    return (
      <div style={{ padding: 16, fontFamily: "ui-sans-serif, system-ui, -apple-system" }}>
        <h1 style={{ fontSize: 18, fontWeight: 600, marginBottom: 8 }}>UI error</h1>
        <p style={{ opacity: 0.8, marginBottom: 12 }}>
          The app UI failed to render. Check the app logs for a frontend error entry.
        </p>
        {this.state.message ? (
          <pre style={{ whiteSpace: "pre-wrap", background: "rgba(0,0,0,0.05)", padding: 12, borderRadius: 8 }}>
            {this.state.message}
          </pre>
        ) : null}
      </div>
    );
  }
}

