import React from "react";
import ReactDOM from "react-dom/client";
import { invoke } from "@tauri-apps/api/core";

import App from './App';
import './i18n'; // Import i18n config
import "./App.css";
import { initFrontendLogging } from "./utils/frontendLogging";
import { ErrorBoundary } from "./components/common/ErrorBoundary";

// 启动时显式调用 Rust 命令显示窗口
// 配合 visible:false 使用，解决启动黑屏问题
invoke("show_main_window").catch(console.error);

initFrontendLogging();

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <ErrorBoundary>
      <App />
    </ErrorBoundary>

  </React.StrictMode>,
);
