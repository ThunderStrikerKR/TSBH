import test from "node:test";
import assert from "node:assert/strict";
import {readFileSync} from "node:fs";
const read=path=>readFileSync(new URL("../"+path,import.meta.url),"utf8");

test("Native catalog and management command routing",()=>{
  const dispatcher=read("src-tauri/src/business_core/mod.rs");
  const native=read("src-tauri/src/business_core/catalog_clients.rs");
  assert.match(dispatcher, /"catalog_clients" => catalog_clients::execute/);
    assert.match(source,new RegExp('"catalog_clients", "'+command+'"'));
  const nativeBrands=read("src-tauri/src/business_core/catalog_clients/brands.rs");
  const nativeCategories=read("src-tauri/src/business_core/catalog_clients/categories.rs");
  assert.match(nativeBrands,/fn upsert_brand\(/);
  assert.match(nativeCategories,/fn change\(/);
  assert.match(nativeCategories,/fn delete\(/);
  assert.match(nativeCategories,/fn legacy_add\(/);
  assert.match(nativeCategories,/fn legacy_rename\(/);
  const source=read("src-tauri/src/business_core/catalog_clients/official_import.rs");
  assert.match(source,/fn source_matches/);
  assert.match(source,/!product_url_ok\(source_url\)/);
  assert.match(source,/미리보기 후 상품 데이터가 변경되었습니다/);
  for(const command of [
    "preview_client_rows","preview_registration_rows",
    "preview_product_sync_rows",
  ])assert.match(native,new RegExp('"'+command+'"'));
});

test("Native snapshot save validates clients and brands",()=>{
  const save=read("src-tauri/src/lib.rs");
  assert.match(save,/fn state_save\(/);
  assert.match(save,/validate_clients\(&current, &state\)\?/);
  assert.match(save,/validate_brands\(&current, &state\)\?/);
});

test("Native catalog search and editor routing",()=>{
  const source=read("src-tauri/src/business_core/catalog_clients/queries.rs");
  const editor=read("src-tauri/src/business_core/catalog_clients/editor.rs");
  assert.match(editor,/fn prepare\(/);
  assert.match(source,/"resolution_product_candidates"/);
  for(const command of [
    "client_candidates","search_products","classified_products",
    "representative_products","suggest_group_representative","official_links",
  ])assert.match(source,new RegExp('"'+command+'"'));
});

test("active alias editor, SKU model group save and official HTML preview are native",()=>{
  const backend=read("src-tauri/src/business_core/catalog_clients.rs");
  const html=read("src-tauri/src/business_core/catalog_clients/official_html.rs");
  const settings=read("crates/bando-data-access/src/settings_commands.rs");
  assert.match(settings,/validate_native_model_groups\(&state\)\?/);
  assert.match(html,/Html::parse_document\(html\)/);
  assert.match(html,/fn option_evidence\(/);
  assert.match(html,/fn model_code\(/);
  for(const command of [
    "save_alias","alias_problems","validate_model_groups",
    "preview_official_product_html",
  ])assert.match(backend,new RegExp('"'+command+'"'));
});
