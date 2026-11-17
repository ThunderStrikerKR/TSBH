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
  assert.match(linked, /onMouseEnter=\{\(\) => c\.setActive\(key\)\}/);
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
  assert.match(idle, /회색: 분류된 원문/);

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

test("actual order desk links shipping memo and visual rules distinguish exact fields from shipment context", () => {
  const desk = read("src/OrderDesk.tsx");
  const source = read("src/SourceDocument.tsx");
  const linked = read("src/LinkedText.tsx");
  const css = read("src/styles/receipt-document.css");
  assert.match(desk, /<Linked linkKey=\{r\.id \+ ":note"\}>/);
  assert.match(desk, /linked-group-context/);
  assert.match(source, /spans=\{sourceDocumentSpans\(order\.analysisRaw \?\? order\.raw, order\)\}/);
  assert.match(linked, /c\.active === key \? " linked-active" : ""/);
  assert.match(linked, /linked-recipient-context/);
  assert.match(css, /\.linked-token\.linked-recipient-context:not\(\.linked-active\)/);
  assert.match(css, /\.linked-token\.linked-active/);
  assert.match(css, /\.recipient-review\.linked-group-context/);
});
