import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
const src = name => readFileSync(new URL("../"+name,import.meta.url),"utf8");

test("product search asks Rust for both eligible products and allowed detail-page links",()=>{
  const catalog=src("src-tauri/src/business_core/catalog_clients.rs");
  const queries=src("src-tauri/src/business_core/catalog_clients/queries.rs");
  assert.match(catalog,/"search_products_with_urls"/);
  assert.match(queries,/"search_products_with_urls"\s*=>/);
  assert.match(queries,/super::product_url_ok/);
  assert.match(catalog,/fn product_url_ok/);
});

test("native enriched search retains the original Rust product filtering contract",()=>{
  const query=src("src-tauri/src/business_core/catalog_clients/queries.rs");
  assert.match(query,/"search_products"=>ids\(filtered_product_search\(state,payload\)/);
  assert.match(query,/let candidates=filtered_product_search\(state,payload\)/);
  assert.match(query,/fn detailed_search_preserves_order_and_uses_catalog_url_validator\(\)/);
});
