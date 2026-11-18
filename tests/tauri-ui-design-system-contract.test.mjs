import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { spawnSync } from "node:child_process";

const read = (file) => readFileSync(new URL("../" + file, import.meta.url), "utf8");
const contract = JSON.parse(read("src/ui/design-system.json"));

test("the authoritative cross-screen UI contract exists inside app source", () => {
  assert.equal(contract.schemaVersion, 1);
  assert.deepEqual(contract.screens.map(({ id }) => id), [
    "접수", "주문내역", "공장목록", "결산", "상품", "운임", "거래처", "설정",
  ]);
  for (const role of ["action-primary", "action-standard", "action-quiet",
    "action-danger", "action-icon", "nav-main", "nav-section", "nav-subsection"]) {
    const detail = contract.controlRoles[role];
    assert.ok(detail && detail.height && detail.fontSize && detail.radius, role);
  }
  for (const kind of ["input", "tableHeader", "tableRow", "card", "modal",
    "status", "toolbar", "notification"]) {
    assert.ok(contract.componentRoles[kind], kind);
  }
});

test("Kakao settings are three independent pane-local buttons, not a cycle", () => {
  const settings = contract.operatingRules.pendingKakaoToolbar;
  assert.equal(settings.status, "approved-awaiting-ui-change");
  assert.deepEqual(settings.buttons.map(({ label }) => label),
    ["함께 보기", "나누어 보기", "카카오톡 연결 해제"]);
  assert.equal(new Set(settings.buttons.map(({ id }) => id)).size, 3);
  assert.match(settings.interaction, /Never use a cycling/);
  assert.match(settings.constraint, /outside the native overlay region/);
  assert.match(read("docs/ui-design-system.md"), /순환 구조가 아니다/);
});

test("UI role validator checks live CSS tokens and must pass contract-only mode", () => {
  const validator = read("scripts/check-ui-design.mjs");
  assert.match(validator, /ts\.isJsxOpeningElement/);
  assert.match(validator, /data-ui-role/);
  assert.match(validator, /UI_DIFF_BASE/);
  const env = { ...process.env };
  delete env.UI_DIFF_BASE;
  const run = spawnSync(process.execPath, ["scripts/check-ui-design.mjs"], {
    cwd: new URL("..", import.meta.url), env, encoding: "utf8",
  });
  assert.equal(run.status, 0, run.stderr || run.stdout);
  assert.match(run.stdout, /UI_DESIGN_CONTRACT_PASS/);
});

test("app developer contract points to the persistent design system", () => {
  assert.match(read("AGENTS.md"), /docs\/ui-design-system\.md/);
  assert.match(read("AGENTS.md"), /src\/ui\/design-system\.json/);
  assert.match(read("AGENTS.md"), /check-ui-design\.mjs/);
  assert.match(read("AGENTS.md"), /기존 화면 스타일 일괄 변경은 따로 명령받기 전 금지/);
  const pkg = JSON.parse(read("package.json"));
  assert.equal(pkg.scripts["check:ui-design"], "node scripts/check-ui-design.mjs");
});
