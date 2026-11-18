#!/usr/bin/env node
// BANDO HUB UI design-system gate. No screen style changes are made here.
// In CI, UI_DIFF_BASE forces changed JSX controls to declare a semantic role.
import assert from "node:assert/strict";
import { readFileSync, existsSync } from "node:fs";
import { execFileSync } from "node:child_process";
import path from "node:path";
import ts from "typescript";

const root = process.cwd();
const contract = JSON.parse(readFileSync(path.join(root, "src/ui/design-system.json"), "utf8"));
const css = readFileSync(path.join(root, "src/styles/tokens.css"), "utf8");
const roleIds = new Set(Object.keys(contract.controlRoles));
const cases = [];
function token(name, value) {
  const marker = "--ds-" + name + ": " + value + ";";
  assert.ok(css.includes(marker), "CSS token missing or inconsistent: " + marker);
  cases.push(marker);
}
assert.equal(contract.schemaVersion, 1);
assert.equal(contract.screens.length, 8);
assert.equal(new Set(contract.screens.map((page) => page.id)).size, 8);
assert.deepEqual(contract.spacing, [4, 8, 12, 16, 24]);
for (const [name, value] of Object.entries(contract.palette)) token("color-" + name, value);
for (const [name, value] of Object.entries(contract.typeRoles)) {
  token("type-" + name + "-size", value.fontSize + "px");
  token("type-" + name + "-weight", String(value.fontWeight));
}
for (const [name, value] of Object.entries(contract.controlRoles)) {
  token(name + "-height", value.height + "px");
  token(name + "-font-size", value.fontSize + "px");
  token(name + "-pad-x", value.horizontalPadding + "px");
  token(name + "-radius", value.radius + "px");
  assert.ok(contract.palette[value.surface] !== undefined, name + ": unknown surface");
  assert.ok(contract.palette[value.text] !== undefined, name + ": unknown text color");
  assert.ok(value.height >= 30 && value.height <= 44, name + ": implausible control height");
}
for (const [name, properties] of Object.entries(contract.componentRoles)) {
  for (const [property, value] of Object.entries(properties)) {
    if (typeof value === "number") token(name + "-" + property, value + "px");
  }
}
for (const [name, value] of Object.entries(contract.tradeTierColors)) {
  token("tier-" + name + "-background", value.background);
  token("tier-" + name + "-foreground", value.foreground);
}
for (const size of contract.spacing) token("space-" + size, size + "px");
for (const [name, value] of Object.entries(contract.shapes)) token(name, value + "px");

const navSource = readFileSync(path.join(root, "src/app/navigation.ts"), "utf8");
for (const screen of contract.screens)
  assert.ok(navSource.includes('id: "' + screen.id + '"'), "Unclassified app screen: " + screen.id);
const kakao = contract.operatingRules.pendingKakaoToolbar;
assert.equal(kakao.buttons.length, 3);
assert.deepEqual(kakao.buttons.map((item) => item.label),
  ["함께 보기", "나누어 보기", "카카오톡 연결 해제"]);
assert.ok(kakao.interaction.includes("Never use a cycling"));
for (const item of kakao.buttons) assert.ok(roleIds.has(item.role), "Kakao unclassified button");

function git(...args) {
  return execFileSync("git", args, { cwd: root, encoding: "utf8", stdio: ["ignore", "pipe", "pipe"] }).trim();
}
function openingControls(filename, content) {
  const source = ts.createSourceFile(filename, content, ts.ScriptTarget.Latest, true, ts.ScriptKind.TSX);
  const entries = [];
  const visit = (node) => {
    if (ts.isJsxOpeningElement(node) || ts.isJsxSelfClosingElement(node)) {
      const element = node.tagName.getText(source);
      if (element === "button" || element === "Button") {
        const attributes = node.attributes.properties.filter(ts.isJsxAttribute);
        const attribute = (name) => attributes.find((entry) => entry.name.text === name);
        const valueOf = (name) => {
          const initializer = attribute(name)?.initializer;
          return initializer && ts.isStringLiteral(initializer) ? initializer.text : undefined;
        };
        const position = source.getLineAndCharacterOfPosition(node.getStart(source));
        entries.push({
          element,
          signature: node.getText(source).replace(/\s+/g, " ").trim(),
          role: valueOf("data-ui-role"),
          variant: valueOf("variant"),
          location: filename + ":" + (position.line + 1),
        });
      }
    }
    ts.forEachChild(node, visit);
  };
  visit(source);
  return entries;
}
const base = process.env.UI_DIFF_BASE?.trim();
let changedControls = 0;
if (base) {
  // The workflow checks out the base SHA too; avoid silent role-check bypass.
  assert.match(base, /^[0-9a-f]{7,40}$/i, "UI_DIFF_BASE must be a Git SHA");
  git("cat-file", "-e", base + "^{commit}");
  const files = git("diff", "--name-only", "--diff-filter=ACMRT", base, "HEAD", "--", "src")
    .split("\n").filter((file) => file.endsWith(".tsx"));
  const errors = [];
  for (const filename of files) {
    if (!existsSync(path.join(root, filename))) continue;
    let original = "";
    try { original = git("show", base + ":" + filename); } catch { /* Newly created view. */ }
    const previous = openingControls(filename, original);
    const counts = new Map();
    for (const row of previous)
      counts.set(row.signature, (counts.get(row.signature) ?? 0) + 1);
    const current = openingControls(filename,
      readFileSync(path.join(root, filename), "utf8"));
    for (const row of current) {
      const originalCount = counts.get(row.signature) ?? 0;
      if (originalCount > 0) {
        counts.set(row.signature, originalCount - 1);
        continue;
      }
      changedControls++;
      if (row.role && roleIds.has(row.role)) continue;
      if (row.element === "Button" &&
        ["primary", "secondary", "danger", "quiet"].includes(row.variant)) continue;
      errors.push(row.location + ": new/edited " + row.element +
        " must declare data-ui-role from src/ui/design-system.json " +
        "(or an explicit shared Button variant)");
    }
  }
  assert.equal(errors.length, 0, "UI design gate:\n" + errors.join("\n"));
}
console.log("UI_DESIGN_CONTRACT_PASS", JSON.stringify({
  roles: roleIds.size, screens: contract.screens.length, cssTokens: cases.length,
  changedControls, diffChecked: Boolean(base)
}));
