# Round 114 — retire orphaned Tauri developer-mode CI

Round 102 removed the application launch path and DEV_WINDOWS_TAURI.cmd. The remaining DEV-mode smoke workflow and probes require that removed launcher, --bando-developer-test, or workspaceMode/TEST SQLite. Delete the dedicated workflow, routing and GUI probes, and developer-only test blocks in mixed legacy contracts. Keep general Tauri window/release tests, shared Rust recognition/pricing/freight/factory/SQLite tests, synthetic business fixtures, current DEV_WINDOWS_EGUI.cmd, and historical records. Do not alter application code, operational DB, or visual evidence rules. The retired workflow's historical runs remain historical failures; do not mark them PASS. Required-check settings could not be inspected with the current connector (403); no settings changed. Code search yielded no results even for known paths; scope verified by named paths and inspected workflow/test references, not a guarantee of zero repository-wide references.

Removed paths:
- `.github/workflows/dev-windows-smoke.yml`
- `tests/ci-dev-domain-gui-contract.test.mjs`
- `tests/ci-windows-smoke-routing.test.mjs`
- `tests/dev-window-placement-real-restart.test.mjs`
- `scripts/classify-dev-windows-smoke.mjs`
- `scripts/ci-dev-domain-gui.mjs`
- `scripts/ci-domains/clients.mjs`
- `scripts/ci-domains/factory.mjs`
- `scripts/ci-domains/orders.mjs`
- `scripts/ci-domains/pricing.mjs`
- `scripts/ci-domains/shipping.mjs`
- `scripts/migration32-dev-windows-smoke.ps1`
- `scripts/verify-dev-window-placement-win.ps1`
- `scripts/verify-window-placement-win.ps1`
- `scripts/migration32-dev-business-ui.mjs`
- `scripts/migration31-real-two-orders.mjs`
- `scripts/migration31-real-account-copy.mjs`
- `scripts/migration31-real-freight-focus.mjs`
- `scripts/migration32-real-factory-notifications.mjs`
- `scripts/migration32-real-ocr-cancel.mjs`
- `scripts/migration32-win-ocr-escape.ps1`
- `scripts/final-windows-full-screen-audit.mjs`
- `scripts/final-windows-populated-tables.mjs`
