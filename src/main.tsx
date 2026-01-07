import React from "react";
import ReactDOM from "react-dom/client";
import { isTauri, request } from "./utils/request";

import App from './App';
import './i18n'; // Import i18n config
import "./App.css";

// 启动时显式调用 Rust 命令显示窗口 (仅 Tauri 模式)
// 配合 visible:false 使用，解决启动黑屏问题
if (isTauri) {
  request("show_main_window").catch(console.error);
}

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <App />

  </React.StrictMode>,
);

