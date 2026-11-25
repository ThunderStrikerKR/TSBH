# Round 102 — Stage 2, batch 8: coverage gap audit

Audit baseline: 432ca957f7c924fe1f1ef3b5ffe14e2f6cc36ecc. Scope: the eight previously observed operational workflows plus the new manual Windows egui package workflow; the prior Node job log; selected current native source paths and stage 2 batch 1–7 records. No production logic, workflow gate or test removed by this audit.

## Evidence and limits

The prior PR #61 head 838195c4 has eight completed successful workflows (run IDs 45534082787, 45534082791, 45534082788, 45534082768, 45534082770, 45534082744, 45534082750, 45534082760). PR #61 remains unmerged and mergeable=false at inspection. Those results predate the CI retirement and cannot validate current main. The available workflow-run lookup filters to pull_request events only; the latest direct-main commit returns no runs. Main push/dispatch execution, actual results and Windows package execution are UNKNOWN, not PASS or FAIL. Do not remove additional coverage before verifying the new native tests.

The old node-validation log contains 821 Subtest occurrences and 808 distinct titles. A machine-readable title inventory is retained in legacy-node-test-inventory.json. Classification is a keyword-based triage only: 393 business candidates, 351 unclassified, 34 legacy UI candidates, 25 migration candidates, 5 developer candidates. Titles can overlap or omit context; all 808 require source/coverage review, not automatic deletion. The old 306 passing Rust tests are not individually re-reviewed here. Other historical workflow files outside the nine known active/manual definitions are not exhaustively inventoried.

## Coverage gaps (open)

| ID | Priority | Gap | Evidence / required closure |
|---|---|---|---|
| G01 | P0 | New CI result unavailable; no current native PASS | Obtain main push/dispatch run IDs, inspect failed jobs, fix tests or real defects, record exact commit evidence. |
| G02 | P0 | Client workbook binary boundary | Native client_preview row validation exists; egui client_manager.rs showed no workbook import/export calls. Verify actual supported feature, then real V1/V2 XLSX read/write, formula/phone/zip-code cells, hidden IDs, backup + CAS. Do not silently restore obsolete UI. |
| G03 | P0 | Incoming factory XLSX to SQLite | egui excel_service.rs writes outbound XLSX; gateway exposes FactoryReplyReviewStatuses but no incoming apply operation in the reviewed enum. Existing rules tests cover matching and application on JSON only. Confirm current approved input route; test Q/G columns, duplicate rows, conflict approval, replay and persistent transaction. |
| G04 | P1 | History/settlement XLSX | Native calculation tests exist; current daily_settlement_manager.rs has no workbook references, order_history_manager.rs has date controls but no confirmed spreadsheet export implementation. Confirm approved scope before implementation; preserve filtered rows and separation of cash/charges if required. |
| G05 | P0 | Restore failure after copying backup | Shared backup tests cover preflight rejection, WAL restore and ordinary transaction rollback, not injected mid-restore failure and automatic recovery. Add targeted test without actual user data. |
| G06 | P1 | Windows actual runtime and OCR | Manual workflow added, not executed. Compiled release EXE is not launched; real UI, clipboard capture, in-flight cancellation, learning and clean-machine startup not proven. |
| G07 | P1 | Product workbook breadth | egui product_excel.rs uses native calamine reader and has formula guard/roundtrip test; older full-catalog, duplicated ID, price tier, archived row and brand/category cases need body-level mapping and missing cases. |
| G08 | P0 | Developer mode residue | egui main.rs:62/9622 still invokes developer_ui_samples; business_state.rs:16–57,80–93,126–137 retains debug/test path selection; excel_service.rs:26ff has developer archive path; src-tauri/src/lib.rs:8,1142–1148 retains developer_workspace commands. Full Windows --lib CI still compiles/tests that module. Remove only after separating isolation/restore protections and confirming current policy. |
| G09 | P1 | Old tests unreviewed | Title inventory is not 808 function-body review. 351 unclassified titles, all candidate classes need mapping to current implementation and explicit expected outcomes. |
| G10 | P0 | PR #61 integration | Prior head passes old CI, not merged. Synchronize using current main CI definitions and preserve both sets of legitimate changes, verify merge on new head. |
| G11 | P1 | Prior discovered correctness candidates | shipping.rs:134 uses q != 0; settlement.rs:76–78 skips missing ship without clearing complete. Confirm reachability and fix only reproduced defects. |
| G12 | P1 | Gate coverage timing | egui-targeted-check broad job excludes PR; freight-sqlite-verify has no PR trigger. Product manager test explicitly skipped. Audit required checks and test expectations before redesign. |
| G13 | P2 | Duplicate checks | Exact duplicate reception_workflow_admission_integration_tests command in egui-targeted-check; manual-classification workflow repeats it and other egui tests. Full output crate embeds rules/recognition modules and subsequent explicit native rules filters may rerun identical source tests; measure exact cross-crate duplication before removal. |
| G14 | P1 | Gate/test-only change distinction | Visual evidence workflow watches egui-app/src/**/*.rs including test-only edits; no fake approval. Review source-vs-test scope while preserving real UI proof. Node runtime for static egui gates is legitimate, not a React dependency. |

## Workflow disposition

- verify-tauri.yml: shared native backup + reception kept; full Tauri library suite still mixed; G08.
- migration-output.yml: standalone Rust output, dispatcher, recognition and rules scheduled; cross-crate duplication G13.
- egui-targeted-check.yml: keep current UI/data contracts; main-only broad job, explicit skip, duplicated call G12/G13.
- freight-sqlite-verify.yml: keep scoped DB and actual gateway fixture; main-only G12.
- client-name-migration-safety.yml: native rows/aliases retained, binary boundary G02.
- egui-manual-classification-check.yml: native UI contract preserved, overlaps G13.
- egui-state-layout.yml: preserve current static Rust layout gate.
- ui-visual-evidence.yml: preserve actual human proof, test-only scope G14.
- egui-portable-acceptance.yml: manually triggered candidate build, G06; not a regular commit gate.

## Exit decision

Stage 2 implementation batches 1–7 are committed, and batch 8 audit evidence is recorded. Coverage migration is NOT complete and CI acceptance is BLOCKED/UNKNOWN. First obtain current CI outcomes; then address P0 gaps and clarify unported feature scope; only then consolidate duplicate tests and restructure gates. No new branch or actual business DB accessed.
