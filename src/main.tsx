import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import { isTauri } from "./lib/env";
import "./styles/global.css";

// Tauri 环境下给 <html> 加 mica 类：body 背景转为透明，让窗口云母材质透出
if (isTauri()) {
  document.documentElement.classList.add("mica");
}

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>
);
