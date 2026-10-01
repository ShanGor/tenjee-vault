import "./App.css";
import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import { bootstrapDocumentPreferences, PreferencesProvider } from "./i18n";
import { assertActionRegistry } from "./shared/actions";

assertActionRegistry();
bootstrapDocumentPreferences();

// The notes router requires its basename even on a fresh desktop launch.
if (!window.location.hash) {
  let lastPath = "/notes";
  try {
    const saved = localStorage.getItem("tenjee-vault:last-path");
    if (saved?.startsWith("/")) lastPath = saved;
  } catch { /* Use the default route when storage is unavailable. */ }
  window.history.replaceState(null, "", `#${lastPath}`);
}

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode><PreferencesProvider><App /></PreferencesProvider></React.StrictMode>,
);
