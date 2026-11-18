import test from "node:test";
import assert from "node:assert/strict";
import {existsSync,readFileSync} from "node:fs";
const file=name=>new URL("../"+name,import.meta.url);
test("orphaned price workbook UI and obsolete native adapter are retired",()=>{
  for(const path of [
    "src/ProductPriceWorkbookTools.tsx",
    "src/native-price-workbook.ts",
    "src/product-price-xlsx.ts",
    "src-tauri/src/business_core/catalog_clients/price_workbook.rs",
  ]) assert.equal(existsSync(file(path)),false,path);
  assert.match(readFileSync(file("tests/rust-migration/oracles/product-price-xlsx.ts"),"utf8"),/previewPriceWorkbook/);
  const view=readFileSync(file("src/ProductSyncWorkbookTools.tsx"),"utf8");
  assert.match(view,/previewNativeUnifiedProductWorkbook/);
  assert.match(view,/"apply_product_workbook"/);
});
