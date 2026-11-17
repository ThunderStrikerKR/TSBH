import assert from "node:assert/strict";
import test from "node:test";
import { readFileSync } from "node:fs";
const read=(p)=>readFileSync(new URL("../"+p,import.meta.url),"utf8");
test("source help icon overrides heading button padding and stays center-aligned",()=>{
  const css=read("src/styles/help-tooltip.css");
  assert.match(css,/\.source-help-heading \.help-tooltip-trigger/);
  assert.match(css,/padding: 0; margin: 0;/);
  assert.match(css,/align-items: center; justify-content: center/);
});
test("no-recipient instructions use the same empty-state base class as other screens",()=>{
  assert.match(read("src/OrderDesk.tsx"),/className="empty detail-empty">발주 원문을 입력하거나 배송 건을 선택하세요/);
  assert.match(read("src/styles/receipt-layout.css"),/\.receipt-panel-content > \.detail-empty/);
});

test("1080p narrow order panes keep the request-message label on one line without changing typography", () => {
  const css = read("src/styles/receipt-document.css");
  const layout = read("src/styles/receipt-layout.css");
  const narrow = read("src/styles/workflow-states.css");
  const desk = read("src/OrderDesk.tsx");
  assert.match(desk, /<Field label="요청 메시지">/);
  assert.match(layout, /\.compact-review \.recipient-fields \.field\s*\{/);
  assert.match(narrow, /@container \(max-width:420px\)/);
  assert.match(css, /\.hub-panels \.compact-review \.recipient-fields > \.linked-field > \.field\s*\{\s*grid-template-columns: 84px minmax\(0, 1fr\);/);
  assert.match(css, /\.hub-panels \.compact-review \.recipient-fields > \.linked-field > \.field > \.field-label\s*\{\s*white-space: nowrap;/);
  assert.match(css, /\.hub-panels \.compact-review \.recipient-fields > \.linked-field\s*\{\s*min-width: 0;\s*width: 100%;/);
});
