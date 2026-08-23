import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import "./styles/app.css";
import { matchLang } from "./i18n";

// Paint in the right palette from the first frame. `settings.json` is the real
// source of truth, but it is a round trip away; this only covers the gap.
(() => {
  let pref: string | null = null;
  try {
    pref = localStorage.getItem("theme");
  } catch {
    // Storage can be refused; the OS setting is a fine answer on its own.
  }
  const dark = window.matchMedia("(prefers-color-scheme: dark)").matches;
  document.documentElement.dataset.theme =
    pref === "light" || pref === "dark" ? pref : dark ? "dark" : "light";

  // Same idea for the language, so the first paint is not in the wrong one.
  let lang: string | null = null;
  try {
    lang = localStorage.getItem("language");
  } catch {
    // Ignored for the same reason as above.
  }
  document.documentElement.lang =
    lang && lang !== "system" ? lang : matchLang(navigator.language);
})();

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
