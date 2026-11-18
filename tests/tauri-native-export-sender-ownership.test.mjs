import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

const source = path => readFileSync(new URL("../" + path, import.meta.url), "utf8");

test("Excel sender-settings guidance uses the Rust export authority", () => {
  const ui = source("src/WorkflowQueue.tsx");
  const native = source("src-tauri/src/business_core/output/mod.rs");
  const exportRules = source("src-tauri/src/business_core/output/export.rs");

  assert.match(ui, /"output", "review_export_sender"/);
  assert.match(ui, /const companyMissing = currentSenderReview\?\.companyMissing === true/);
  assert.doesNotMatch(ui, /companyMissing\s*=\s*chosen\.some/);
  assert.doesNotMatch(ui, /!sender\?\.(?:name|address|phone|mobile)/);
  assert.match(native, /"review_export_sender"\s*=>\s*export::sender_review/);
  assert.match(exportRules, /fn sender_complete\(sender: &Value\) -> bool/);
  assert.match(exportRules, /if !sender_complete\(sender\)/);
  assert.match(exportRules, /pub\(super\) fn sender_review/);
});

test("sender guidance does not record a stale Rust answer as a current result", () => {
  const ui = source("src/WorkflowQueue.tsx");
  assert.match(ui, /let cancelled = false/);
  assert.match(ui, /if \(!cancelled\) setSenderReview/);
  assert.match(ui, /return \(\) => \{ cancelled = true; \}/);
  assert.match(ui, /senderReview\?\.source === state/);
  assert.match(ui, /senderReview\.selection === senderSelectionKey/);
});
