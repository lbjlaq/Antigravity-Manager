import React from "react";
import ReactDOM from "react-dom/client";
import { invoke } from "@tauri-apps/api/core";

import App from './App';
import './i18n'; // Import i18n config
import "./App.css";

function tryShowMainWindow() {
  let attempts = 0;
  const maxAttempts = 80; // ~4s

  const tick = () => {
    attempts += 1;
    invoke("show_main_window")
      .then(() => {})
      .catch(() => {
        if (attempts >= maxAttempts) return;
        setTimeout(tick, 50);
      });
  };

  tick();
}

// Startup: window is configured as visible:false, so we must show it once the IPC bridge is ready.
tryShowMainWindow();

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <App />

  </React.StrictMode>,
);
