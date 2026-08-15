import React from "react";
import ReactDOM from "react-dom/client";

import { App } from "./app/App";
import { installDeploymentRecovery } from "./app/deploymentRecovery";
import { bootstrapUiStore } from "./stores/uiStore";
import "./design/fonts.css";
import "./styles/globals.css";

installDeploymentRecovery();

// Restore persisted UI state before any component reads the store.
bootstrapUiStore();

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>
);
