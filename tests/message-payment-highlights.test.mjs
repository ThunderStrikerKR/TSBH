import assert from "node:assert/strict";
import test from "node:test";
import { tsImport } from "tsx/esm/api";

const { paymentMessageHighlights, selectedAccountFromSignature } = await tsImport(
  "../src/message-payment-highlights.ts", import.meta.url,
);
const account = (bank, number, holder) => ({ bank, number, holder });
const values = (text, selected) => paymentMessageHighlights(text, selected)
  .map(({ kind, start, end }) => ({ kind, text: text.slice(start, end) }));

for (const vat of ["부가세 별도", "부가세 포함", "부가세 포함/별도"]) {
  test("highlights complete bank, account number and holder as one span: " + vat, () => {
    const text = [
      "김가상님", "BD 709 * 1개 : 78,000원", "운임 : 8,000원", "",
      "합계 : 86,000원 입니다. (" + vat + ")", "",
      "제일은행 24141421424 강명재 입니다.",
    ].join("\n");
    assert.deepEqual(values(text, account("제일은행", "24141421424", "강명재")), [
      { kind: "total", text: "86,000원" },
      { kind: "vat", text: "(" + vat + ")" },
      { kind: "account", text: "제일은행 24141421424 강명재" },
    ]);
  });
}
test("custom account separators are included but polite sentence suffix is excluded", () => {
  const text = "청구합니다 123,400원 (부가세 포함)\n\n입금 계좌: 농협 / 313-1111-1111 / 이가상 님 감사합니다.";
  assert.deepEqual(values(text, account("농협", "313-1111-1111", "이가상")), [
    { kind: "total", text: "123,400원" },
    { kind: "vat", text: "(부가세 포함)" },
    { kind: "account", text: "농협 / 313-1111-1111 / 이가상" },
  ]);
});
test("reordered fields still make one highlight in a custom template", () => {
  const text = "합계 15,000원 (부가세 별도)\n\n예금주 홍가상 | 은행 제일은행 | 번호 1212-1111 입니다.";
  assert.deepEqual(values(text, account("제일은행", "1212-1111", "홍가상")), [
    { kind: "total", text: "15,000원" },
    { kind: "vat", text: "(부가세 별도)" },
    { kind: "account", text: "홍가상 | 은행 제일은행 | 번호 1212-1111" },
  ]);
});
test("missing account or total must not highlight earlier prices, shipping or phone numbers", () => {
  const text = "상품 78,000원\n연락처 010-1111-2222\n\n합계 : 금액 확인 필요 (부가세 별도)\n\n[입금 계좌 등록 필요]";
  assert.deepEqual(values(text, null), [{ kind: "vat", text: "(부가세 별도)" }]);
});
test("partial or mismatched account must not highlight unrelated or earlier values", () => {
  const text = "제일은행 12345 홍가상\n\n합계 : 22,000원 (부가세 별도)\n\n제일은행 12345 다른 사람";
  assert.deepEqual(values(text, account("제일은행", "12345", "홍가상")), [
    { kind: "total", text: "22,000원" }, { kind: "vat", text: "(부가세 별도)" },
  ]);
});
test("signature parses all three Rust-selected account fields without reselecting", () => {
  const signature = JSON.stringify({
    text: "Rust generated copy",
    account: { id: "a1", bank: "가상은행", number: "100-200-300", holder: "예금주" },
    items: [],
  });
  assert.deepEqual(selectedAccountFromSignature(signature), account("가상은행", "100-200-300", "예금주"));
  assert.equal(selectedAccountFromSignature(JSON.stringify({ text: "no account" })), null);
  assert.equal(selectedAccountFromSignature(JSON.stringify({ account: { bank: "은행", number: "123" } })), null);
  assert.equal(selectedAccountFromSignature("{invalid"), null);
});
