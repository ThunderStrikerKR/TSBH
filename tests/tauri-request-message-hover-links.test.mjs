import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import { tsImport } from "tsx/esm/api";

const { requestMessageSpans } = await tsImport(
  "../src/request-message-links.ts", import.meta.url,
);
const read = (path) => readFileSync(new URL("../" + path, import.meta.url), "utf8");
const recipient = (id, name, items) => ({
  id, name, phone: "", mobile: "", address: "",
  items: items.map(([id, name]) => ({ id, name })),
});
const order = (...recipients) => ({ id: "o1", recipients });
const contains = (spans, text, targetId, field, term) =>
  spans.some((s) => s.targetId === targetId && s.field === field &&
    text.slice(s.start, s.end).includes(term));

test("linked source hover activates at 200ms, clears at 250ms, without committing a shipment", () => {
  const linked = read("src/LinkedText.tsx");
  const hub = read("src/HubWorkspace.tsx");
  const desk = read("src/OrderDesk.tsx");
  const hook = read("src/orders/useOrderDesk.tsx");
  assert.match(linked, /beginHover\?: \(key: string\) => void/);
  assert.match(linked, /clearHover\?: \(\) => void/);
  assert.match(linked, /onMouseEnter=\{\(\) => \(c\.beginHover \?\? c\.setActive\)\(key\)\}/);
  assert.match(linked, /onMouseEnter=\{\(\) => \(c\.beginHover \?\? c\.setActive\)\(linkKey\)\}/);
  assert.match(hub, /linkHoverOnTimer\.current = setTimeout\(\(\) => \{[\s\S]*?setActive\(key\);[\s\S]*?\}, 200\)/);
  assert.match(hub, /if \(linkHoverOnTimer\.current\) clearTimeout\(linkHoverOnTimer\.current\)/);
  assert.equal((linked.match(/c\.clearHover \?\? \(\(\) => c\.setActive\(""\)\)/g) ?? []).length, 2);
  assert.match(hub, /linkHoverOffTimer\.current = setTimeout\(\(\) => \{[\s\S]*?setActive\(""\);[\s\S]*?\}, 250\)/);
  assert.match(hub, /if \(linkHoverOffTimer\.current\) clearTimeout\(linkHoverOffTimer\.current\)/);
  const setter = hub.split("  const setActiveLive = ")[1]?.split("  const clearLinkHoverLive = ")[0] ?? "";
  assert.doesNotMatch(setter, /setSelectedLive|commitSelection/);
  assert.match(hub, /const linkedHoverShipment = active && previewReady/);
  assert.match(hub, /const visibleSelection = hoverSelection \?\? linkedHoverSelection \?\? selected/);
  assert.match(desk, /clearHover: linkController\?\.clearHover/);
  assert.match(hub, /beginHover: beginLinkHoverLive/);
  assert.match(hub, /setActive: setActiveLive/);
  assert.match(hook, /clearHover\?: \(\) => void/);
});

test("default request hover links missing-address/contact, item quantity, model and name to the actual field", () => {
  const data = order(recipient("r1", "김민수", [
    ["i1", "BD 709"], ["i2", "BD 708"],
  ]));
  const text = "1. 김민수님: 배송지 주소, 연락처, BD 709 수량, BD 708 모델 / 옵션\n\n위 내용 확인 부탁드립니다.";
  const spans = requestMessageSpans(text, data);
  assert.ok(contains(spans, text, "r1", "name", "김민수"));
  assert.ok(contains(spans, text, "r1", "address", "배송지 주소"));
  assert.ok(contains(spans, text, "r1", "phone", "연락처"));
  assert.ok(contains(spans, text, "i1", "quantity", "BD 709 수량"));
  assert.ok(contains(spans, text, "i2", "product", "BD 708 모델 / 옵션"));
  assert.ok(spans.every((s) => text.slice(s.start, s.end) === s.text));
});

test("hovering multiple recipients with identical product labels never highlights the wrong person's row", () => {
  const data = order(
    recipient("r1", "김민수", [["i1", "BD 709"]]),
    recipient("r2", "이가상", [["i2", "BD 709"]]),
  );
  const text = "1. 김민수님: BD 709 수량, 연락처\n\n2. 이가상님: BD 709 수량, 상세주소\n\n위 내용 확인 부탁드립니다.";
  const spans = requestMessageSpans(text, data);
  const firstEnd = text.indexOf("\n\n");
  const secondStart = text.indexOf("2. 이가상님");
  const first = spans.filter((s) => s.start < firstEnd);
  const second = spans.filter((s) => s.start >= secondStart);
  assert.ok(contains(first, text, "i1", "quantity", "BD 709 수량"));
  assert.ok(contains(second, text, "i2", "quantity", "BD 709 수량"));
  assert.ok(contains(first, text, "r1", "phone", "연락처"));
  assert.ok(contains(second, text, "r2", "address", "상세주소"));
  assert.ok(first.every((s) => s.recipientId === "r1"));
  assert.ok(second.every((s) => s.recipientId === "r2"));
});

test("missing recipient name and a custom request text also target the editable input", () => {
  const data = order(recipient("r1", "", [["i1", "BD 709"]]));
  const text = "1. 수취인님: 수취인명, 연락처\n\n위 내용 확인 부탁드립니다.";
  const spans = requestMessageSpans(text, data);
  assert.ok(contains(spans, text, "r1", "name", "수취인명"));
  assert.ok(contains(spans, text, "r1", "phone", "연락처"));
  const named = order(recipient("r2", "김민수", [["i2", "BD 709"]]));
  const edited = "김민수님 연락처 확인 부탁드려요.\n\n김민수님 BD 709 상품으로 보내드리면 될까요?";
  const custom = requestMessageSpans(edited, named);
  assert.ok(contains(custom, edited, "r2", "phone", "연락처"));
  assert.ok(contains(custom, edited, "i2", "product", "BD 709"));
});

test("an ambiguous recipient identity is never guessed from a shared name or a generic field label", () => {
  const data = order(
    recipient("r1", "홍길동", [["i1", "BD 709"]]),
    recipient("r2", "홍길동", [["i2", "BD 709"]]),
  );
  assert.deepStrictEqual(requestMessageSpans("1. 홍길동님: 연락처", data), []);
  assert.deepStrictEqual(requestMessageSpans("연락처 확인 부탁드려요.", data), []);
});

test("actual message preview passes its phase to the hover mapper, and hover events set the linked field key", () => {
  const view = read("src/MessageWorkspace.tsx");
  const linked = read("src/LinkedText.tsx");
  const desk = read("src/OrderDesk.tsx");
  const hub = read("src/HubWorkspace.tsx");
  assert.match(view, /messageSpans\(text, order, request \? "request" : "payment"\)/);
  assert.match(linked, /if \(phase === "request"\) return requestMessageSpans\(text, order\)/);
  assert.match(linked, /onMouseEnter=\{\(\) => \(c\.beginHover \?\? c\.setActive\)\(key\)\}/);
  assert.match(linked, /c\.active === linkKey \? "linked-active" : ""/);
  assert.match(desk, /<Linked linkKey=\{r\.id \+ ":address"\}>/);
  assert.match(desk, /<Linked linkKey=\{r\.id \+ ":phone"\}>/);
  assert.match(desk, /<Linked linkKey=\{i\.id \+ ":quantity"\}>/);
  assert.match(desk, /<Linked linkKey=\{i\.id \+ ":product"\}>/);
  assert.match(hub, /<LinkContext\.Provider value=\{linkController\}>/);
  assert.match(linked, /\(directEdit \? editing : true\)/);
  assert.match(linked, /강조 보기/);
  assert.match(linked, /useState\(!text && !directEdit\)/);
  assert.match(view, /key=\{order\.id \+ ":" \+ \(gid \?\? ""\)/);
});

test("real React request preview starts as hoverable linked text after async data, not a textarea", async () => {
  const React = await import("react");
  const { renderToStaticMarkup } = await import("react-dom/server");
  const { LinkedText, LinkContext } = await tsImport("../src/LinkedText.tsx", import.meta.url);
  const data = order(recipient("r1", "김민수", [["i1", "BD 709"]]));
  const text = "1. 김민수님: 연락처, BD 709 수량";
  const render = (value, active) => renderToStaticMarkup(
    React.createElement(LinkContext.Provider, {
      value: { active, recipientId: "r1", setActive: () => {} },
    }, React.createElement(LinkedText, {
      text: value, spans: requestMessageSpans(value, data),
      label: "복사용 자동 메시지 본문",
      directEdit: true, readOnly: false, onChange: () => {},
    })),
  );
  const initiallyEmpty = render("", "");
  assert.doesNotMatch(initiallyEmpty, /<textarea/);
  const shown = render(text, "r1:phone");
  assert.doesNotMatch(shown, /<textarea/);
  assert.match(shown, /data-link-key="r1:phone"[^>]*linked-active/);
  assert.match(shown, /data-link-key="i1:quantity"/);
});

test("classified source spans remain grey until exact-field selection, with separate recipient context", async () => {
  const React = await import("react");
  const { renderToStaticMarkup } = await import("react-dom/server");
  const { LinkedText, LinkContext, sourceDocumentSpans } = await tsImport(
    "../src/LinkedText.tsx", import.meta.url,
  );
  const text = "709 1개\n김가상\n배송메모: 문 앞\n\n이민수\n배송메모: 경비실";
  const note1 = text.indexOf("문 앞");
  const note2 = text.indexOf("경비실");
  const name1 = text.indexOf("김가상");
  const name2 = text.indexOf("이민수");
  const order = { recipients: [
    { id: "r1", name: "김가상", note: "문 앞", items: [], sources: [
      { start: name1, end: name1 + 3, text: "김가상", field: "name", targetId: "r1", recipientId: "r1" },
      { start: note1, end: note1 + 3, text: "문 앞", field: "note", targetId: "r1", recipientId: "r1" },
    ] },
    { id: "r2", name: "이민수", note: "경비실", items: [], sources: [
      { start: name2, end: name2 + 3, text: "이민수", field: "name", targetId: "r2", recipientId: "r2" },
      { start: note2, end: note2 + 3, text: "경비실", field: "note", targetId: "r2", recipientId: "r2" },
    ] },
  ] };
  const spans = sourceDocumentSpans(text, order);
  assert.equal(spans.length, 4);
  assert.equal(text.slice(spans[1].start, spans[1].end), "배송메모: 문 앞");
  assert.equal(text.slice(spans[3].start, spans[3].end), "배송메모: 경비실");
  assert.equal(spans[1].field, "note");
  assert.equal(spans[1].targetId + ":" + spans[1].field, "r1:note");

  const render = (active, recipientId = "r1") => renderToStaticMarkup(
    React.createElement(LinkContext.Provider, {
      value: { active, recipientId, setActive: () => {} },
    }, React.createElement(LinkedText, {
      text, spans, label: "주문 원문", readOnly: true, onChange: () => {},
    })),
  );
  const tag = (markup, key) => {
    const match = markup.match(new RegExp('<span[^>]*data-link-key="' + key + '"[^>]*>'));
    assert.ok(match, "missing linked token: " + key);
    return match[0];
  };
  const idle = render("");
  assert.doesNotMatch(tag(idle, "r1:note"), /linked-active/);
  assert.match(tag(idle, "r1:note"), /linked-recipient-context/);
  assert.doesNotMatch(tag(idle, "r2:note"), /linked-recipient-context/);
  assert.doesNotMatch(idle, /회색: 분류된 원문|source-link-legend/);

  const active = render("r1:note");
  assert.match(tag(active, "r1:note"), /linked-active/);
  assert.doesNotMatch(tag(active, "r1:name"), /linked-active/);
  assert.doesNotMatch(tag(active, "r2:note"), /linked-active/);
  assert.match(tag(active, "r1:name"), /linked-recipient-context/);
  assert.match(tag(active, "r2:note"), /linked-token/);
  assert.equal((active.match(/linked-active/g) ?? []).length, 1);
});

test("a recognized empty shipping memo label remains a hover target without inventing content", async () => {
  const { sourceDocumentSpans } = await tsImport("../src/LinkedText.tsx", import.meta.url);
  const text = "김가상\n배송메모 :\n";
  const at = text.indexOf("배송메모") + "배송메모 :".length;
  const result = sourceDocumentSpans(text, { recipients: [{
    id: "r1", items: [], sources: [
      { start: at, end: at, text: "", field: "note", recipientId: "r1", targetId: "r1" },
    ],
  }] });
  assert.equal(result.length, 1);
  assert.equal(text.slice(result[0].start, result[0].end), "배송메모 :");
  assert.equal(result[0].field, "note");
});

test("order desk links shipping memo without a duplicate header name or grey detail overlay", () => {
  const desk = read("src/OrderDesk.tsx");
  const source = read("src/SourceDocument.tsx");
  const linked = read("src/LinkedText.tsx");
  const css = read("src/styles/receipt-document.css");
  assert.match(desk, /<Linked linkKey=\{r\.id \+ ":note"\}>/);
  assert.match(desk, /<h2>수취인별 주문 정보<\/h2>/);
  assert.doesNotMatch(desk, /<b>\{draft\.recipients\.find\(\(r\) => r\.id === shownRecipient\)\?\.name\}<\/b>/);
  assert.match(desk, /<Field label="이름">/);
  assert.doesNotMatch(desk, /linked-group-context/);
  assert.match(source, /spans=\{sourceDocumentSpans\(order\.analysisRaw \?\? order\.raw, order\)\}/);
  assert.match(linked, /c\.active === key \? " linked-active" : ""/);
  assert.match(linked, /linked-recipient-context/);
  assert.match(css, /\.linked-token\.linked-recipient-context:not\(\.linked-active\)/);
  assert.match(css, /\.linked-token\.linked-active/);
  assert.doesNotMatch(css, /\.recipient-review\.linked-group-context/);
});

test("source chips have no permanent border; selected fields use four-sided blue fill and stroke", () => {
  const css = read("src/styles/receipt-document.css");
  const source = css.split("/* Classified source spans")[1]?.split("/* Recipient ownership is already represented")[0] ?? "";
  const details = css.split("/* Highlight the actual input's inner box")[1] ?? "";
  assert.ok(source, "source highlight contract missing");
  assert.match(source, /\.linked-token\s*\{[\s\S]*?border: 0;/);
  assert.match(source, /\.linked-token\.linked-active\s*\{[\s\S]*?background: #17436d !important;[\s\S]*?box-shadow: inset 0 0 0 2px #75b5ff !important;/);
  assert.doesNotMatch(source, /border: 2px solid|border-color:|box-shadow: inset 0 -2px/);
  assert.match(details, /\.linked-field\.linked-active\s*\{[\s\S]*?outline: none !important;/);
  assert.match(details, /\.linked-field\.linked-active \.field :is\(input, select, textarea\)\s*\{[\s\S]*?background: #17436d !important;[\s\S]*?color: #eaf4ff !important;[\s\S]*?box-shadow: inset 0 0 0 2px #75b5ff !important;/);
  assert.doesNotMatch(details, /outline: 2px solid #75b5ff/);
});

test("native item product expressions include 민판 and parenthesized color, but not quantity", async () => {
  const { sourceDocumentSpans } = await tsImport("../src/LinkedText.tsx", import.meta.url);
  const examples = [
    { raw: "709민판 2개", model: "709", product: "709민판", quantity: "2개" },
    { raw: "709(블랙) 4개", model: "709", product: "709(블랙)", quantity: "4개" },
    { raw: "반도 뮤직트레이 (그레이) 1개", model: "뮤직트레이",
      product: "반도 뮤직트레이 (그레이)", quantity: "1개" },
    { raw: "709민판", model: "709", product: "709민판", quantity: null },
  ];
  const text = examples.map((e) => e.raw).join("\n");
  const recipients = examples.map((e, index) => {
    const start = text.split("\n").slice(0, index).join("\n").length + (index ? 1 : 0);
    const modelStart = start + e.raw.indexOf(e.model);
    const qtyStart = e.quantity ? start + e.raw.lastIndexOf(e.quantity) : -1;
    const id = "r" + index;
    const itemId = "i" + index;
    return { id, name: "", address: "", items: [{
      id: itemId, raw: e.raw, sources: [
        { start: modelStart, end: modelStart + e.model.length,
          text: e.model, field: "product", targetId: itemId, recipientId: id },
        ...(e.quantity ? [{ start: qtyStart, end: qtyStart + e.quantity.length,
          text: e.quantity, field: "quantity", targetId: itemId, recipientId: id }] : []),
      ],
    }], sources: [] };
  });
  const spans = sourceDocumentSpans(text, { recipients });
  for (const [index, e] of examples.entries()) {
    const itemId = "i" + index;
    const product = spans.find((s) => s.targetId === itemId && s.field === "product");
    assert.ok(product, "product evidence exists");
    assert.equal(text.slice(product.start, product.end), e.product);
    assert.equal(product.text, e.product);
    assert.equal(product.recipientId, "r" + index);
    const qty = spans.find((s) => s.targetId === itemId && s.field === "quantity");
    if (e.quantity) {
      assert.ok(qty, "separate quantity evidence remains available");
      assert.equal(text.slice(qty.start, qty.end), e.quantity);
      assert.ok(product.end < qty.start, "product cue must not steal quantity");
    } else assert.equal(qty, undefined);
  }
});

test("product visual extension must not capture independently linked recipient names", async () => {
  const { sourceDocumentSpans } = await tsImport("../src/LinkedText.tsx", import.meta.url);
  const text = "709민판 김가상";
  const spans = sourceDocumentSpans(text, { recipients: [{
    id: "r1", sources: [
      { start: text.indexOf("김가상"), end: text.length, text: "김가상",
        targetId: "r1", recipientId: "r1", field: "name" },
    ], items: [{
      id: "i1", raw: text, sources: [
        { start: 0, end: 3, text: "709", targetId: "i1", recipientId: "r1", field: "product" },
      ],
    }],
  }] });
  assert.equal(spans.find((s) => s.field === "product")?.text, "709");
  assert.equal(spans.find((s) => s.field === "name")?.text, "김가상");
});

test("neutral source highlights remain filled chips without grey detail-card overlays", () => {
  const css = read("src/styles/receipt-document.css");
  const source = css.split("/* Classified source spans")[1]?.split("/* Recipient ownership is already represented")[0] ?? "";
  assert.match(source, /\.linked-token\s*\{[\s\S]*?background: #424242;[\s\S]*?border: 0;[\s\S]*?border-radius: 4px;[\s\S]*?padding: 1px 3px;/);
  assert.match(source, /-webkit-box-decoration-break: clone;/);
  assert.doesNotMatch(source, /border: 2px solid|border-color:/);
  assert.doesNotMatch(css, /\.recipient-review\.linked-group-context/);
  assert.doesNotMatch(css, /box-shadow: inset 3px 0 0 #777/);
});

test("product and price buttons plus quantity input share the blue selection fill and outline", () => {
  const css = read("src/styles/receipt-document.css");
  const desk = read("src/OrderDesk.tsx");
  const activeItem = css.split("/* Item, quantity and amount are the same linked-field state")[1] ?? "";
  assert.match(desk, /<Linked linkKey=\{i\.id \+ ":product"\}>/);
  assert.match(desk, /<Linked linkKey=\{i\.id \+ ":quantity"\}>/);
  assert.match(desk, /<Linked linkKey=\{i\.id \+ ":price"\}>/);
  assert.match(activeItem, /\.linked-field\.linked-active\s*\{[\s\S]*?outline: none !important;[\s\S]*?box-shadow: none !important;/);
  assert.match(activeItem, /\.linked-field\.linked-active > :is\(button, input, select, textarea\)\s*\{[\s\S]*?background: #17436d !important;[\s\S]*?color: #eaf4ff !important;[\s\S]*?box-shadow: inset 0 0 0 2px #75b5ff !important;/);
});

test("quantity values align to the right without changing quantity stepper controls", () => {
  const css = read("src/styles/patch-v0808.css");
  const detail = read("src/OrderDesk.tsx");
  const manual = read("src/orders/ManualClassification.tsx");
  assert.match(detail, /type="number"[\s\S]*?aria-label=\{"수량 " \+ i\.name\}/);
  assert.match(manual, /inputMode=\{cell\.field === "quantity" \? "numeric" : undefined\}/);
  assert.match(css, /\.items-freight-panel \.compact-items input\[type="number"\]\s*\{[\s\S]*?text-align: right;/);
  assert.match(css, /\.app-v03 \.quantity-stepper input,[\s\S]*?\.app-v03 \.manual-count input\[type="number"\],[\s\S]*?text-align: right;/);
});
