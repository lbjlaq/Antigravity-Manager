// File: src/main.tsx
// Application entry point

import React from "react";
import ReactDOM from "react-dom/client";
import { invoke } from "@tauri-apps/api/core";

import App from './app/App';
import { isTauri } from '@/shared/lib';
import "./app/styles/global.css";

// Show main window on startup (works with visible:false to fix black screen)
if (isTauri()) {
  invoke("show_main_window").catch(console.error);
}

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
