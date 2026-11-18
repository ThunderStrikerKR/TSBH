import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
const read = (file) => readFileSync(new URL('../' + file, import.meta.url), 'utf8');

test('product price workbook preserves hidden product and variant identity', () => {
  const format = read('tests/rust-migration/oracles/product-price-xlsx.ts');
  const oracle = read('tests/rust-migration/oracles/product-price-xlsx.ts');
  assert.match(format, /__상품ID/);
  assert.match(format, /__가격기록ID/);
  assert.match(oracle, /data\[8\] === \(v\.option/);
  assert.match(format, /LEGACY_PRICE_HEADERS/);
  assert.doesNotMatch(format, /normalizeCode\(data\[8\]\)/);
  assert.match(oracle, /productMappings\?\.\[values\[6\] \+ "\|" \+ values\[7\]\]/);
  assert.match(format, /sheet\.columns\.forEach/);
});
test('XLSX previews parse sheet format in TS and validate prices/history in Rust', () => {
  const code = read('tests/rust-migration/oracles/product-price-xlsx.ts');
  const oracle = read('tests/rust-migration/oracles/product-price-xlsx.ts');
  assert.match(code, /20 \* 1024 \* 1024/);
  assert.match(code, /sheet\.rowCount > 20001/);
  assert.match(code, /hidden|식별자/);
  assert.match(oracle, /priceHistory\.push/);
  assert.match(oracle, /if \(!raw\) return;/);
  assert.match(code, /cell\.isMerged/);
});
test('ExcelJS is loaded only for workbook actions, not application startup', () => {
  const workbook = read('tests/rust-migration/oracles/product-price-xlsx.ts');
  assert.match(workbook, /import type ExcelJS/);
  assert.match(workbook, /await import\("exceljs"\)/);
  assert.doesNotMatch(workbook, /import ExcelJS from/);
});
test('unused standalone price editor is absent from live ProductManager', () => {
  const product = read('src/ProductManager.tsx');
  assert.match(product, /ProductSyncWorkbookTools/);
  assert.doesNotMatch(product, /ProductPriceWorkbookTools/);
});
test('Tauri uses a controlled native XLSX write instead of opening arbitrary paths', () => {
  const rust = read('src-tauri/src/lib.rs');
  assert.match(rust, /fn save_product_workbook/);
  assert.match(rust, /\.create_new\(true\)/);
  assert.match(rust, /save_product_workbook,\s*save_registration_workbook/);
  assert.doesNotMatch(rust, /fn save_product_workbook\([^)]*filename:/);
});
test('Category map is a full-width interactive tree with on-demand editor', () => {
  const component = read('src/CategoryManager.tsx');
  const css = read('src/styles/tauri-stage2-products.css');
  assert.match(component, /catalog-map-row/);
  assert.match(component, /catalog-map-editor-layer/);
  assert.match(component, /setExpanded\(\(old\)/);
  assert.match(css, /\.catalog-admin-workspace\s*\{[\s\S]*?display: block/);
  assert.doesNotMatch(component, /catalog-tree-toggle/);
});
test('Category direct product transfer asks for consent only after add begins', () => {
  const component = read('src/CategoryManager.tsx');
  assert.match(component, /operation === "add"/);
  assert.match(component, /transferConfirmed/);
  assert.match(component, /catalog-add-transfer/);
  assert.doesNotMatch(component, /연결 상품이 있습니다\. 하위 분류 생성 전에/);
});
test('Product detail is neutral dark and registration button is not duplicated', () => {
  const product = read('src/ProductManager.tsx');
  const css = read('src/styles/tauri-stage2-products.css');
  assert.equal((product.match(/<RegistrationManager\s/g) || []).length, 1);
  assert.match(css, /product-detail-dialog > \.modal\s*\{[\s\S]*?background: #202020/);
  assert.match(read('src/main.tsx'), /tauri-stage2-products\.css/);
});
