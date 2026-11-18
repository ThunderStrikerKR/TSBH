import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
const read=path=>readFileSync(new URL("../"+path,import.meta.url),"utf8");
test("operational price workbook parsing is ExcelJS-only and Rust judges all prices",()=>{
  const view=read("src/ProductPriceWorkbookTools.tsx");
  const native=read("src/native-price-workbook.ts");
  const sheet=read("src/product-price-xlsx.ts");
  const rust=read("src-tauri/src/business_core/catalog_clients.rs");
  assert.match(view,/await previewNativePriceWorkbook\(state, file\)/);
  assert.doesNotMatch(view,/previewPriceWorkbook\(/);
  assert.match(native,/"catalog_clients", "preview_price_workbook_rows"/);
  assert.match(sheet,/readPriceWorkbookRows/);
  assert.doesNotMatch(sheet,/function rowMatches|export async function previewPriceWorkbook/);
  assert.match(rust,/"preview_price_workbook_rows" => price_workbook::preview\(&payload\)/);
  assert.match(read("tests/rust-migration/oracles/product-price-xlsx.ts"),/export async function previewPriceWorkbook/);
});
