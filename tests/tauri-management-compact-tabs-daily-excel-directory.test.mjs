import assert from "node:assert/strict";
import test from "node:test";
import { readFileSync } from "node:fs";
const read = p => readFileSync(new URL("../" + p,import.meta.url),"utf8");

test("native opener validates fixed kinds and uses the same dated archive as saver",()=>{
  const rust=read("src-tauri/src/lib.rs");
  const native=rust.split("fn open_daily_excel_directory(")[1].split("\n}")[0];
  assert.ok(native.includes('"FACTORY" => WorkbookKind::Factory'));
  assert.ok(native.includes('"STORE" => WorkbookKind::Store'));
  assert.ok(native.includes('workspace_excel_directory(&db, &state["settings"])'));
  assert.ok(native.includes("archive_daily_directory(&root, workbook_kind, Local::now())"));
  assert.ok(rust.includes("open_excel_directory, open_daily_excel_directory"));
});
