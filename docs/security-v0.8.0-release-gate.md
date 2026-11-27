# v0.8.0 release security gate (round 170)

Status: BLOCKED / code only. This branch is not merged to main. No operating database, actual OCR runtime, release ZIP or user data was accessed in this review.

## Preconditions on a trusted Windows x64 build computer

- Use an isolated checkout of this branch. Fetch the complete history and all relevant refs; a shallow or partial history does not establish a complete Gitleaks scan. Keep scan output local and redacted. If a leak is found, revoke/rotate the credential and handle any history cleanup separately; never automatically rewrite main.
- Install trusted, independently verified versions of Git, Rust/MSVC, Gitleaks, cargo-audit and pip-audit. The gate checks the tools are present; their own installation provenance is an operator responsibility.
- Generate real lockfiles with `cargo generate-lockfile --manifest-path egui-app/Cargo.toml` and `cargo generate-lockfile --manifest-path crates/bando-data-access/Cargo.toml`; review and commit both on this branch. Do not substitute the Tauri lockfile. Release is blocked until both are tracked and the checkout is clean.
- Prepare the OCR inputs on an isolated build machine. Acquire Python embed x64 ZIP, pinned pip.pyz, pinned wheelhouse (all transitive wheels), the exact hash-locked requirements and Korean detection/recognition models from reviewed sources. Verify provenance and hashes independently before approving the input manifest. No hash is supplied here without evidence.
- The offline preparation script accepts BANDO_OCR_INPUT_DIR, BANDO_OCR_INPUT_MANIFEST and BANDO_OCR_INPUT_SHA256. Input root contains python-embed.zip, pip.pyz, requirements.lock, wheels/, models/. Each non-comment lock line must be name==version followed by one or more --hash=sha256:<64 hex> entries on the same line. No remote URLs, unconstrained dependencies or automatic network downloads.
- A prepared OCR candidate is not automatically trusted. Review it, verify the exact full file inventory, and approve the output manifest independently, before running its Python. Do not generate approval automatically during packaging. Keep private approval inputs outside the tree, in a trusted directory not writable by untrusted users. Store the manifest as JSON with schemaVersion=1 and files=[{path:"runtime/example",sha256:"<64 hex>"}]; paths use forward slashes. The example is illustrative, not a valid approval. Obtain the manifest SHA256 independently through a trusted channel. Reject unexpected, missing or modified files, private data patterns and reparse points. The strict file policy may need a narrowly reviewed exception for legitimate package files; do not bypass it.
- Set BANDO_OCR_RUNTIME_DIR to the approved runtime (or use release/ocr), BANDO_OCR_APPROVAL_MANIFEST to the independent manifest path and BANDO_OCR_APPROVAL_SHA256 to its verified hash. Keep approved trees read-only to other users. Run PACKAGE_WINDOWS_EGUI.cmd. Missing approval, failed audit, failed locked build, failed synthetic backup/integrity tests, or failed relocated OCR verification blocks ZIP publication. No automatic download fallback.

## Backup and Windows acceptance

The gate executes synthetic backup/restore regressions using the existing bando-data-access backup_service tests. Those tests do not establish that a real operating backup can be restored. Before deployment, close the current app, back up production data including the separate preview/learning DB and SQLite sidecars as appropriate (prefer the app's verified SQLite backup for the live DB), retain the previous EXE, verify a restore on an isolated copy, and document the outcome without publishing any private files. Do not restore into the live DB as a test. Verify the version, OCR, reception, settings, Excel, factory reply, and settlement on a Windows test system using synthetic data. Deliver the ZIP SHA256 independently. Do not treat a package hash as code signing.

## Seven required items

1. OCR input integrity: offline verified input gate added; independently sourced inputs and approval still missing.
2. OCR cache trust: exact inventory checks before copying, after staging, and after extracting the ZIP; synthetic tests added but not executed in the chat environment.
3. Rust lock: --locked and tracked locks required; actual lockfiles not generated.
4. Dependency scan: cargo-audit and pip-audit gate added; no scan result yet.
5. Git protection: sidecar ignore rules and redacted Gitleaks gate added; actual full-history scan not executed.
6. Backup: synthetic regression gate added; actual test run and isolated production-backup restore not executed.
7. Packaging: gated release build and extraction verification in code; Windows execution not performed.

Release decision: BLOCKED until evidence is collected. Do not merge or publish on the basis of code changes alone.
