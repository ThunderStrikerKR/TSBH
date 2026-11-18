import assert from "node:assert/strict";
import test from "node:test";
import { tsImport } from "tsx/esm/api";

const { paymentMessageHighlights } = await tsImport(
  "../src/message-payment-highlights.ts", import.meta.url,
);
const values = (text, account) => paymentMessageHighlights(text, account)
  .map(({ kind, start, end }) => ({ kind, text: text.slice(start, end) }));

for (const vat of ["부가세 별도", "부가세 포함", "부가세 포함/별도"]) {
  test("payment preview highlights only the grand total, VAT label and Rust-selected number: " + vat, () => {
    const text = [
      "김가상님",
      "BD 709 * 1개 : 78,000원",
      "운임 : 8,000원",
      "",
      "합계 : 86,000원 입니다. (" + vat + ")",
      "",
      "제일은행 24141421424 예금주 입니다.",
    ].join("\n");
    assert.deepEqual(values(text, "24141421424"), [
      { kind: "total", text: "86,000원" },
      { kind: "vat", text: "(" + vat + ")" },
      { kind: "account", text: "24141421424" },
    ]);
  });
}

test("a custom grand-total template is visually parsed without recalculating totals", () => {
  const text = "청구합니다 123,400원 (부가세 포함)\n\n은행명 고객계좌: 313-1111-1111";
  assert.deepEqual(values(text, "313-1111-1111"), [
    { kind: "total", text: "123,400원" },
    { kind: "vat", text: "(부가세 포함)" },
    { kind: "account", text: "313-1111-1111" },
  ]);
});

test("missing account or total never highlights other money and phone numbers", () => {
  const text = "상품 78,000원\n연락처 010-1111-2222\n\n합계 : 금액 확인 필요 (부가세 별도)\n\n[입금 계좌 등록 필요]";
  assert.deepEqual(values(text, ""), [
    { kind: "vat", text: "(부가세 별도)" },
  ]);
});

test("the exact Rust bank number must appear after the VAT summary", () => {
  const text = "계좌 비슷한 번호 12345\n\n합계 : 22,000원 (부가세 별도)\n\n다른 계좌 33333";
  assert.deepEqual(values(text, "12345"), [
    { kind: "total", text: "22,000원" },
    { kind: "vat", text: "(부가세 별도)" },
  ]);
});
