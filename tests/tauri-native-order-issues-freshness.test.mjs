import assert from "node:assert/strict";
import test from "node:test";
import {readFileSync} from "node:fs";
test("complete multi-line Korean address does not become a choice of fragments",()=>{
  const rust=readFileSync(new URL("../src-tauri/src/business_core/rules/issues.rs",import.meta.url),"utf8");
  assert.match(rust,/fn address_candidates_form_one_location/);
  assert.match(rust,/field_name=="address" &&\s*address_candidates_form_one_location\(row,&candidates\)/);
  assert.match(rust,/multiline_province_road_and_floor_are_one_complete_address/);
  assert.match(rust,/two_independent_complete_addresses_still_require_review/);
  assert.match(rust,/conflicting_administrative_fragments_are_not_auto_approved/);
});
