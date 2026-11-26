import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
const read = (path) => readFileSync(new URL("../" + path, import.meta.url), "utf8");

test("native validation holds only untrustworthy rows and still enforces safe matching", () => {
  const rust=read("src-tauri/src/business_core/rules/factory_reply.rs");
  assert.doesNotMatch(rust,/if field\(source,"trackingNumeric"\)==true\{\s*return Err/);
  assert.doesNotMatch(rust,/return Err\(format!\("\{n\}행: 표준 양식의 J열/);
  assert.match(rust,/numeric_risk/);
  assert.match(rust,/source_issue/);
  assert.match(rust,/if !tracking\.is_empty\(\) && s\(row,"issue"\)\.is_empty\(\)/);
  assert.match(rust,/trackingNumberFormat/);
  assert.match(rust,/zero_padded/);
  assert.match(rust,/fn row_matches/);
  assert.match(rust,/ids\.len\(\)!=1/);
  assert.match(rust,/기존 송장 \/ 실제 운임과 충돌합니다/);
});
