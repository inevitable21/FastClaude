import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import "./index.css";

// Apply persisted theme before React mounts so there is no flash.
// `index.html` ships with class="dark"; only flip if the user previously chose light.
const stored = localStorage.getItem("fastclaude-theme");
if (stored === "light") {
  document.documentElement.classList.remove("dark");
}

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
