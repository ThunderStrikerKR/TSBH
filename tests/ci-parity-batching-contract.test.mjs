import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
const yaml=readFileSync(new URL("../.github/workflows/migration-output.yml",import.meta.url),"utf8");
test("every pre-batch Rust/TypeScript parity command is retained",()=>{
  const commands=[
    "cargo test --manifest-path tests/rust-migration/output/Cargo.toml",
    "cargo test --manifest-path tests/rust-migration/integration/Cargo.toml -- --nocapture",
    "node --test tests/rust-migration/rules/order-save-parity.test.mjs",
    "node --test tests/rust-migration/rules/shipping-rate-parity.test.mjs",
    "node --test tests/rust-migration/rules/preview-issues-parity.test.mjs",
    "npx tsx tests/rust-migration/rules/parity/compare.mts",
    "node --test tests/rust-migration/rules/history-parity.test.mjs",
    "node --test tests/rust-migration/output/parity.test.mjs",
    "node --test tests/rust-migration/output/payment-parity.test.mjs",
    "node --test tests/rust-migration/output/automatic-request-parity.test.mjs",
    "node --test tests/rust-migration/output/full-automatic-parity.test.mjs",
    "node --test tests/rust-migration/output/correction-parity.test.mjs",
    "node --test tests/rust-migration/catalog-clients/title-clients-parity.test.mjs",
    "node --test tests/rust-migration/rules/billing-commit-parity.test.mjs",
    "node --test tests/rust-migration/rules/manual-apply-parity.test.mjs",
    "node --test tests/rust-migration/output/factory-review-parity.test.mjs",
    "node --test tests/rust-migration/rules/shipping-read-parity.test.mjs",
    "node --test tests/rust-migration/rules/parity.test.mjs",
    "node --test tests/rust-migration/recognition/original-oracle-seal.test.mjs",
    "node --test tests/rust-migration/recognition/react-reception.test.mjs",
    "node --test tests/rust-migration/rules/order-edit-native-parity.test.mjs",
    "cargo test --manifest-path tests/rust-migration/recognition/Cargo.toml hongcheon_school_reception_keeps_two_phones_and_three_order_quantities -- --nocapture",
    "node --test tests/rust-migration/factory-reply/parity.test.mjs",
    "node --test tests/tauri-factory-reply-import.test.mjs tests/tauri-factory-reply-auto.test.mjs"
  ];
  for(const command of commands){
    assert.equal(yaml.split("        run: "+command+"\n").length-1,1,"Missing or duplicated: "+command);
  }
});
test("single npm install per parity batch and no redundant Windows parser runner",()=>{
  for(const job of ["rules-parity-batch","output-reception-factory-parity-batch"])
    assert.ok(yaml.includes("  "+job+":"));
  assert.doesNotMatch(yaml,/^  distribution-script-syntax:$/m);
  assert.match(yaml,/shell: pwsh[\s\S]*?ParseFile\(/);
  assert.equal((yaml.match(/run: npm ci --no-audit --no-fund --ignore-scripts/g)||[]).length,3);
  assert.match(yaml,/report\.length !== 10/);
});

test("Windows-only harness changes avoid unrelated TS/Rust parity CI",()=>{
  const onlyWindows=["tests/ci-windows-smoke-routing.test.mjs","tests/ci-dev-domain-gui-contract.test.mjs","tests/dev-window-placement-real-restart.test.mjs","tests/tauri-window-placement.test.mjs","scripts/ci-dev-domain-gui.mjs","scripts/migration32-dev-business-ui.mjs","scripts/migration31-real-two-orders.mjs","scripts/migration32-real-factory-notifications.mjs","scripts/migration32-real-ocr-cancel.mjs","scripts/migration32-dev-windows-smoke.ps1","scripts/classify-dev-windows-smoke.mjs","scripts/verify-dev-window-placement-win.ps1"];
  for(const path of onlyWindows){
    const needle="      - "+String.fromCharCode(39)+"!"+path+String.fromCharCode(39);
    assert.equal(yaml.split(needle).length-1,2,"Expected push and PR exclusion: "+path);
  }
});
