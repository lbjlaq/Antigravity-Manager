import React from "react";
import ReactDOM from "react-dom/client";

import App from './App';
import './i18n'; // Import i18n config
import "./App.css";

// Web 版不需要手动显示窗口

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
