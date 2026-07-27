import React from "react";
import ReactDOM from "react-dom/client";

import { App } from "./app/App";
import { bootstrapUiStore } from "./stores/uiStore";
import "./styles/globals.css";

// Restore persisted UI state before any component reads the store.
bootstrapUiStore();

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>
);
