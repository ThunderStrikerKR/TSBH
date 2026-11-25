import "./tauri-bridge";
import React from "react";
import { createRoot } from "react-dom/client";
import { App } from "./App";
import { AuditProfiler } from "./perf-audit-profiler";
import { disableInputHistorySuggestions } from "./shared/disable-input-history";
import "./styles/index.css";
import "./styles/v0809.css";
import "./styles/product-admin-redesign.css";
import "./styles/tauri-stage2-products.css";
import "./styles/table-density.css";
import "./styles/management-layout.css";
  disableInputHistorySuggestions(document.body);
  createRoot(document.getElementById("root")!).render(
    <React.StrictMode>
      <AuditProfiler id="AppRoot"><App /></AuditProfiler>
    </React.StrictMode>,
  );
