import assert from "node:assert/strict";
import test from "node:test";
import { readFileSync } from "node:fs";
import { tsImport } from "tsx/esm/api";

const { parseEvidence } = await tsImport("../src/parser.ts", import.meta.url);
const state = JSON.parse(readFileSync(
  new URL("../src-tauri/resources/state-seed.json", import.meta.url), "utf8"));
const client = {
  id: "postal-test", name: "가상 거래처", active: true, tier: "wholesale",
  payment: "PREPAID", identifiers: "", overrides: {},
  shippingFeeExcluded: false,
};
const address = "제주특별자치도 서귀포시 대정읍 테스트로 23 (대정읍, 가상아파트) 105동 403호";

for (const postal of ["[63644]", "(63644)", "63644", "[663644]"]) {
  test("labelled postal marker is not the address: " + postal, () => {
    // Never put real customer details into test fixtures.
    const raw = [
      "안녕하세요 직발송 부탁드립니다.",
      "BD 709 (블랙) 1개",
      "수령자: 김가상",
      "휴대전화: 010-0000-0000",
      "주소 : " + postal + " " + address,
    ].join("\n");
    const { recipients, candidates } = parseEvidence(raw, client, state);
    assert.equal(recipients.length, 1);
    assert.equal(recipients[0].address, address);
    assert.equal(recipients[0].addressParts?.province, "제주특별자치도");
    assert.equal(recipients[0].addressParts?.detail, "(대정읍, 가상아파트) 105동 403호");
    const source = recipients[0].sources?.find(x => x.field === "address");
    assert.equal(source?.text, address);
    assert.equal(source?.start, raw.indexOf(address));
    const evidence = candidates.find(x => x.kind === "address");
    assert.equal(evidence?.text, address);
    assert.equal(evidence?.value, address);
    assert.ok(raw.includes(postal), "Raw source must remain unmodified");
  });
}

test("postal marker alone must not become a confirmed street address", () => {
  const raw = "수령자: 김가상\n주소: [63644]";
  const { recipients } = parseEvidence(raw, client, state);
  assert.ok(recipients.every(row => row.address !== "[63644]"));
});
