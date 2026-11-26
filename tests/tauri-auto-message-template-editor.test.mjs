import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
const source = path => readFileSync(new URL("../"+path,import.meta.url),"utf8");

test("editing templates does not modify sale prices or historical orders",()=>{
  const native=source("crates/bando-data-access/src/settings_commands.rs");
  const validation=source("crates/bando-data-access/src/validation.rs");
  assert.match(native,/"automaticMessageTemplates"/);
  assert.match(validation,/validate_message_templates\(s\)\?/);
  assert.match(native,/before!=&expected\[field\]\s*&&\s*before!=updated/);
});
