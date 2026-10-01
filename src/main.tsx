import "./App.css";
import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import { bootstrapDocumentPreferences, PreferencesProvider } from "./i18n";
import { assertActionRegistry } from "./shared/actions";

assertActionRegistry();
bootstrapDocumentPreferences();

// The notes router requires its basename even on a fresh desktop launch.
if (!window.location.hash) window.history.replaceState(null, "", "#/notes");

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode><PreferencesProvider><App /></PreferencesProvider></React.StrictMode>,
);
